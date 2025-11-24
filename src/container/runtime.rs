use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nix::sys::signal::{kill, Signal};
use nix::unistd::{chdir, execv, fork, setgid, sethostname, setuid, ForkResult, Gid, Pid, Uid};
use nix::sys::wait::WaitStatus as NixWaitStatus;
use nix::sys::wait::waitpid as nix_waitpid;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use super::{Container, ContainerStatus, NetworkMode};
use crate::container::container_store as store;
use crate::container::image_store::ImageStore;
use crate::container::rootfs::RootfsBuilder;
use crate::error::{CuboError, Result};
use crate::container::namespace as ns;

pub struct ContainerRuntime {
    containers: Arc<Mutex<HashMap<String, Container>>>,
    root_dir: PathBuf,
    config: RuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub root_dir: PathBuf,
    pub default_network_mode: NetworkMode,
    pub debug: bool,
    pub container_timeout: u64,
}

#[derive(Debug)]
pub struct ExecutionContext {
    pub container: Container,
    pub rootfs_path: PathBuf,
    pub detach: bool,
}

impl ContainerRuntime {
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        if !config.root_dir.exists() {
            fs::create_dir_all(&config.root_dir)
                .map_err(|e| CuboError::SystemError(format!("Failed to create root directory: {}", e)))?;
        }

        let mut loaded: HashMap<String, Container> = store::load_all(&config.root_dir)?;

        for container in loaded.values_mut() {
            if matches!(container.status, ContainerStatus::Running) {
                if !store::pid_is_alive(container.pid) {
                    container.update_status(ContainerStatus::Stopped);
                    let _ = store::save_state(&config.root_dir, container);
                }
            }
        }

        Ok(Self {
            containers: Arc::new(Mutex::new(loaded)),
            root_dir: config.root_dir.clone(),
            config,
        })
    }

    pub async fn create_container(&self, container: Container) -> Result<String> {
        let container_id = container.id.clone();

        let container_dir = self.root_dir.join(&container_id);
        fs::create_dir_all(&container_dir)
            .map_err(|e| CuboError::SystemError(format!("Failed to create container directory: {}", e)))?;

        let rootfs_dir = container_dir.join("rootfs");
        fs::create_dir_all(&rootfs_dir)
            .map_err(|e| CuboError::SystemError(format!("Failed to create rootfs directory: {}", e)))?;

        self.setup_rootfs(&container, &rootfs_dir)?;

        store::save_config(&self.root_dir, &container)?;
        store::save_state(&self.root_dir, &container)?;

        let mut containers = self.containers.lock().await;
        containers.insert(container_id.clone(), container);

        info!("Created container: {}", container_id);
        Ok(container_id)
    }

    pub async fn start_container(&self, container_id: &str, detach: bool) -> Result<()> {
        let mut containers = self.containers.lock().await;
        let container = containers.get_mut(container_id)
            .ok_or_else(|| CuboError::ContainerNotFound(container_id.to_string()))?;

        if container.is_running() {
            return Err(CuboError::SystemError("Container is already running".to_string()));
        }

        container.update_status(ContainerStatus::Running);
        let container_snapshot = container.clone();
        drop(containers);
        store::save_state(&self.root_dir, &container_snapshot)?;

        let exec_ctx = ExecutionContext {
            container: container_snapshot.clone(),
            rootfs_path: self.root_dir.join(container_id).join("rootfs"),
            detach,
        };

        let container_id_clone = container_id.to_string();
        let runtime = self.clone();

        if detach {
            tokio::spawn(async move {
                if let Err(e) = runtime.run_container_process(exec_ctx).await {
                    error!("Container {} failed: {}", container_id_clone, e);
                    runtime.set_container_status(&container_id_clone, ContainerStatus::Error).await;
                }
            });
        } else {
            self.run_container_process(exec_ctx).await?;
        }

        Ok(())
    }

    pub async fn stop_container(&self, container_id: &str, timeout: Option<Duration>) -> Result<()> {
        let mut containers = self.containers.lock().await;
        let container = containers.get_mut(container_id)
            .ok_or_else(|| CuboError::ContainerNotRunning(container_id.to_string()))?;

        if !container.is_running() {
            return Ok(());
        }

        if let Some(pid) = container.pid {
            let timeout = timeout.unwrap_or(Duration::from_secs(10));

            if let Err(e) = kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
                warn!("Failed to send SIGTERM to container {}: {}", container_id, e);
            }

            sleep(timeout).await;

            if let Err(e) = kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
                warn!("Failed to send SIGKILL to container {}: {}", container_id, e);
            }
        }

        container.update_status(ContainerStatus::Stopped);
        let snapshot = container.clone();
        info!("Stopped container: {}", container_id);
        drop(containers);
        store::save_state(&self.root_dir, &snapshot)?;
        Ok(())
    }

    pub async fn remove_container(&self, container_id: &str, force: bool) -> Result<()> {
        let mut containers = self.containers.lock().await;
        let container = containers.get(container_id)
            .ok_or_else(|| CuboError::ContainerNotRunning(container_id.to_string()))?;

        if container.is_running() && !force {
            return Err(CuboError::SystemError("Container is running. Use --force to remove".to_string()));
        }

        if container.is_running() {
            drop(containers);
            self.stop_container(container_id, Some(Duration::from_secs(5))).await?;
            containers = self.containers.lock().await;
        }

        let container_dir = self.root_dir.join(container_id);
        if container_dir.exists() {
            fs::remove_dir_all(&container_dir)
                .map_err(|e| CuboError::SystemError(format!("Failed to remove container directory: {}", e)))?;
        }

        containers.remove(container_id);

        info!("Removed container: {}", container_id);
        Ok(())
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<Container>> {
        let containers = self.containers.lock().await;
        let mut result = Vec::new();

        for container in containers.values() {
            if all || container.is_running() {
                result.push(container.clone());
            }
        }

        Ok(result)
    }

    pub async fn get_container(&self, container_id: &str) -> Result<Container> {
        let containers = self.containers.lock().await;
        containers.get(container_id)
            .cloned()
            .ok_or_else(|| CuboError::ContainerNotRunning(container_id.to_string()))
    }

    async fn run_container_process(&self, exec_ctx: ExecutionContext) -> Result<()> {
        let container_id = exec_ctx.container.id.clone();
        
        info!("Starting the container process: {}", container_id);

        let result = self.create_isolated_process(&exec_ctx).await;

        match result {
            Ok(exit_code) => {
                self.set_container_exit_code(&container_id, exit_code).await;
                self.set_container_status(&container_id, ContainerStatus::Stopped).await;
                info!("Container {} exited with code: {}", container_id, exit_code);
            }
            Err(e) => {
                error!("Container {} failed: {}", container_id, e);
                self.set_container_status(&container_id, ContainerStatus::Error).await;
                return Err(e);
            }
        }

        Ok(())
    }

    async fn create_isolated_process(&self, exec_ctx: &ExecutionContext) -> Result<i32> {
        let container = &exec_ctx.container;

        let program = CString::new("/bin/sh")
            .map_err(|e| CuboError::SystemError(format!("Invalid command: {}", e)))?;

        let shell_command = container.command.join(" ");
        let args = vec![
            CString::new("/bin/sh").unwrap(),
            CString::new("-c").unwrap(),
            CString::new(shell_command)
                .map_err(|e| CuboError::SystemError(format!("Invalid command: {}", e)))?,
        ];

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                self.set_container_pid(&container.id, child.as_raw() as u32).await;
                match nix_waitpid(child, None) {
                    Ok(NixWaitStatus::Exited(_, exit_code)) => Ok(exit_code),
                    Ok(NixWaitStatus::Signaled(_, signal, _)) => {
                        warn!("Container {} killed by signal: {:?}", container.id, signal);
                        Ok(128 + signal as i32)
                    }
                    Ok(status) => {
                        warn!("Container {} exited with status: {:?}", container.id, status);
                        Ok(1)
                    }
                    Err(e) => Err(CuboError::SystemError(format!("Failed to wait for child: {}", e))),
                }
            }
            Ok(ForkResult::Child) => {
                if let Err(e) = ns::unshare_user_then_map_ids() {
                    error!("userns setup failed: {}", e);
                    std::process::exit(1);
                }

                if let Err(e) = ns::unshare_mount_pid_net(&container.config.network_mode) {
                    error!("unshare mount/pid/net failed: {}", e);
                    std::process::exit(1);
                }

                match unsafe { fork() } {
                    Ok(ForkResult::Parent { child }) => {
                        loop {
                            match nix_waitpid(child, None) {
                                Ok(NixWaitStatus::Exited(_, code)) => std::process::exit(code),
                                Ok(NixWaitStatus::Signaled(_, sig, _)) => std::process::exit(128 + sig as i32),
                                Ok(NixWaitStatus::StillAlive) => continue,
                                Ok(_) => continue,
                                Err(e) => {
                                    error!("waitpid failed: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                    Ok(ForkResult::Child) => {
                        if let Err(e) = self.setup_namespaced_container(exec_ctx, &program, &args) {
                            error!("Container setup failed: {}", e);
                            std::process::exit(1);
                        }
                        std::process::exit(1);
                    }
                    Err(e) => {
                        error!("fork into pid namespace failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => Err(CuboError::SystemError(format!("Failed to fork: {}", e))),
        }
    }

    fn setup_namespaced_container(&self, exec_ctx: &ExecutionContext, program: &CString, args: &[CString]) -> Result<()> {
        let container = &exec_ctx.container;
        ns::make_mounts_private()?;

        for volume in &container.config.volume_mounts {
            match volume.mount_type {
                super::MountType::Bind => {
                    let target = exec_ctx
                        .rootfs_path
                        .join(volume.container_path.trim_start_matches('/'));
                    let host = std::path::Path::new(&volume.host_path);
                    ns::bind_mount(host, &target, volume.read_only)?;
                }
                super::MountType::Tmpfs => {
                    use nix::mount::{mount, MsFlags};
                    let target = exec_ctx
                        .rootfs_path
                        .join(volume.container_path.trim_start_matches('/'));
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| CuboError::NamespaceError(format!(
                                "Failed to create tmpfs parent {:?}: {}",
                                parent, e
                            )))?;
                    }
                    fs::create_dir_all(&target)
                        .map_err(|e| CuboError::NamespaceError(format!(
                            "Failed to create tmpfs dir {:?}: {}",
                            target, e
                        )))?;
                    mount::<str, std::path::Path, str, str>(
                        Some("tmpfs"),
                        &target,
                        Some("tmpfs"),
                        MsFlags::MS_NODEV | MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
                        None,
                    )
                    .map_err(|e| CuboError::NamespaceError(format!(
                        "Failed to mount tmpfs at {:?}: {}",
                        target, e
                    )))?;
                }
                super::MountType::Volume => {
                    debug!("Named volumes not implemented; skipping mount for {}", volume.container_path);
                }
            }
        }

        ns::pivot_to_rootfs(&exec_ctx.rootfs_path)?;

        if let Some(ref hostname) = container.config.hostname {
            sethostname(hostname)
                .map_err(|e| CuboError::SystemError(format!("Failed to set hostname: {}", e)))?;
        }

        ns::mount_proc()?;

        if !matches!(container.config.network_mode, NetworkMode::Host) {
            let _ = ns::setup_loopback();
        }

        if let Some(ref workdir) = container.config.working_dir {
            chdir(workdir.as_str())
                .map_err(|e| CuboError::SystemError(format!("Failed to change directory: {}", e)))?;
        }

        for (key, value) in &container.config.env_vars {
            std::env::set_var(key, value);
        }

        if let Some(ref user) = container.config.user {
            self.setup_user(user)?;
        }
        if let Some(ref user) = container.config.user {
            self.setup_user(user)?;
        }

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                loop {
                    match nix_waitpid(Pid::from_raw(-1), None) {
                        Ok(NixWaitStatus::Exited(pid, code)) => {
                            if pid == child { std::process::exit(code); }
                        }
                        Ok(NixWaitStatus::Signaled(pid, sig, _)) => {
                            if pid == child { std::process::exit(128 + sig as i32); }
                        }
                        Ok(NixWaitStatus::StillAlive) => continue,
                        Ok(_) => continue,
                        Err(e) => {
                            if let nix::errno::Errno::ECHILD = e {
                                std::process::exit(0);
                            } else {
                                error!("waitpid in pid1 failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
            Ok(ForkResult::Child) => {
                if let Err(e) = execv(program, args) {
                    error!("Failed to execute command: {}", e);
                    std::process::exit(1);
                }
                unreachable!();
            }
            Err(e) => return Err(CuboError::SystemError(format!("PID1 reaper fork failed: {}", e))),
        }
    }

    fn mount_volume(&self, rootfs_path: &Path, volume: &super::VolumeMount) -> Result<()> {
        let container_path = rootfs_path.join(volume.container_path.trim_start_matches('/'));
        
        if let Some(parent) = container_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CuboError::VolumeError(format!("Failed to create mount point: {}", e)))?;
        }
        match volume.mount_type {
            super::MountType::Bind => {
                let host_path = Path::new(&volume.host_path);
                if !host_path.exists() {
                    warn!("Host path does not exist: {}", volume.host_path);
                    return Ok(());
                }

                if host_path.is_dir() {
                    fs::create_dir_all(&container_path)
                        .map_err(|e| CuboError::VolumeError(format!("Failed to create directory: {}", e)))?;
                } else {
                    if let Some(parent) = container_path.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| CuboError::VolumeError(format!("Failed to create parent directory: {}", e)))?;
                    }
                    fs::File::create(&container_path)
                        .map_err(|e| CuboError::VolumeError(format!("Failed to create file: {}", e)))?;
                }

                debug!("Volume mount handled in namespace setup for: {} -> {}", volume.host_path, volume.container_path);
            }
            super::MountType::Tmpfs => {
                fs::create_dir_all(&container_path)
                    .map_err(|e| CuboError::VolumeError(format!("Failed to create tmpfs directory: {}", e)))?;

                debug!("Tmpfs mount simulated for: {}", volume.container_path);
            }
            super::MountType::Volume => {
                fs::create_dir_all(&container_path)
                    .map_err(|e| CuboError::VolumeError(format!("Failed to create directory: {}", e)))?;
                
                debug!("Named volume simulated for: {}", volume.container_path);
            }
        }

        Ok(())
    }

    fn setup_user(&self, user_spec: &str) -> Result<()> {
        let parts: Vec<&str> = user_spec.split(':').collect();
        
        if parts.len() == 1 {
            let uid: u32 = parts[0].parse()
                .map_err(|e| CuboError::SystemError(format!("Invalid UID: {}", e)))?;
            setuid(Uid::from_raw(uid))
                .map_err(|e| CuboError::SystemError(format!("Failed to set UID: {}", e)))?;
        } else if parts.len() == 2 {
            let uid: u32 = parts[0].parse()
                .map_err(|e| CuboError::SystemError(format!("Invalid UID: {}", e)))?;
            let gid: u32 = parts[1].parse()
                .map_err(|e| CuboError::SystemError(format!("Invalid GID: {}", e)))?;
            
            setgid(Gid::from_raw(gid))
                .map_err(|e| CuboError::SystemError(format!("Failed to set GID: {}", e)))?;
            setuid(Uid::from_raw(uid))
                .map_err(|e| CuboError::SystemError(format!("Failed to set UID: {}", e)))?;
        } else {
            return Err(CuboError::SystemError("Invalid user specification".to_string()));
        }

        Ok(())
    }

    fn setup_rootfs(&self, container: &Container, rootfs_path: &Path) -> Result<()> {
        let image_store = ImageStore::new(self.root_dir.join("images"))?;
        let builder = RootfsBuilder::new(&image_store);

        match builder.build_from_image(&container.blueprint, rootfs_path) {
            Ok(_) => {
                info!("Successfully built rootfs from image: {}", container.blueprint);
                Ok(())
            }
            Err(CuboError::BlueprintNotFound(_)) => {
                warn!(
                    "Image {} not found, creating minimal rootfs. Import the image using image_store.import_tar()",
                    container.blueprint
                );
                builder.create_minimal_rootfs(rootfs_path)
            }
            Err(e) => {
                warn!("Failed to build rootfs from image: {}, falling back to minimal rootfs", e);
                builder.create_minimal_rootfs(rootfs_path)
            }
        }
    }

    async fn set_container_status(&self, container_id: &str, status: ContainerStatus) {
        let mut containers = self.containers.lock().await;
        if let Some(container) = containers.get_mut(container_id) {
            container.update_status(status);
            let snapshot = container.clone();
            drop(containers);
            let _ = store::save_state(&self.root_dir, &snapshot);
            return;
        }
    }

    async fn set_container_pid(&self, container_id: &str, pid: u32) {
        let mut containers = self.containers.lock().await;
        if let Some(container) = containers.get_mut(container_id) {
            container.set_pid(pid);
            let snapshot = container.clone();
            drop(containers);
            let _ = store::save_state(&self.root_dir, &snapshot);
            return;
        }
    }

    async fn set_container_exit_code(&self, container_id: &str, exit_code: i32) {
        let mut containers = self.containers.lock().await;
        if let Some(container) = containers.get_mut(container_id) {
            container.set_exit_code(exit_code);
            let snapshot = container.clone();
            drop(containers);
            let _ = store::save_state(&self.root_dir, &snapshot);
            return;
        }
    }
}

impl Clone for ContainerRuntime {
    fn clone(&self) -> Self {
        Self {
            containers: Arc::clone(&self.containers),
            root_dir: self.root_dir.clone(),
            config: self.config.clone(),
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            root_dir: default_root_dir(),
            default_network_mode: NetworkMode::Bridge,
            debug: false,
            container_timeout: 300,
        }
    }
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(root) = std::env::var("CUBO_ROOT") {
            if !root.is_empty() {
                cfg.root_dir = PathBuf::from(root);
            }
        }
        cfg
    }
}


fn default_root_dir() -> PathBuf {
    fn with_leaf(base: PathBuf) -> PathBuf { base.join("cubo") }

    if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        if !state_home.is_empty() {
            return with_leaf(PathBuf::from(state_home));
        }
    }

    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            return with_leaf(PathBuf::from(data_home));
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            let home_path = PathBuf::from(home);
            let state_default = home_path.join(".local").join("state");
            return with_leaf(state_default);
        }
    }

    PathBuf::from("/tmp/cubo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Container, VolumeMount, MountType};
    use crate::container::container_store as store;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_container() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
        );

        let container_id = runtime.create_container(container).await.unwrap();
        assert!(!container_id.is_empty());

        let containers = runtime.list_containers(true).await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, container_id);

        let bundle = temp_dir.path().join(&container_id);
        assert!(bundle.exists());
        assert!(bundle.join("config.json").exists());
        assert!(bundle.join("state.json").exists());
    }

    #[tokio::test]
    async fn test_container_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
        );

        let container_id = runtime.create_container(container).await.unwrap();
        
        let container = runtime.get_container(&container_id).await.unwrap();
        assert_eq!(container.status, ContainerStatus::Created);

        runtime.remove_container(&container_id, false).await.unwrap();
        
        assert!(runtime.get_container(&container_id).await.is_err());

        assert!(!temp_dir.path().join(&container_id).exists());
    }

    #[tokio::test]
    async fn test_reconcile_dead_pid_to_stopped() {
        let temp_dir = TempDir::new().unwrap();
        let mut c = Container::new("demo:latest".into(), vec!["/bin/echo".into(), "hi".into()]);
        c.set_pid(999_999);
        c.update_status(ContainerStatus::Running);
        store::save_config(temp_dir.path(), &c).unwrap();
        store::save_state(temp_dir.path(), &c).unwrap();

        let config = RuntimeConfig { root_dir: temp_dir.path().to_path_buf(), ..Default::default() };
        let rt = ContainerRuntime::new(config).unwrap();

        let loaded = rt.get_container(&c.id).await.unwrap();
        assert_eq!(loaded.status, ContainerStatus::Stopped);
        assert!(loaded.finished_at.is_some());

        let st_path = temp_dir.path().join(&c.id).join("state.json");
        let st: store::OciState = store::read_json(&st_path).unwrap();
        assert_eq!(st.status, "stopped");
    }

    #[test]
    #[serial_test::serial]
    fn test_runtime_config_from_env() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", tmp.path());
        let cfg = RuntimeConfig::from_env();
        assert_eq!(cfg.root_dir, tmp.path());
        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    fn test_runtime_config_default() {
        let cfg = RuntimeConfig::default();
        assert!(!cfg.debug);
        assert_eq!(cfg.container_timeout, 300);
        assert!(matches!(cfg.default_network_mode, NetworkMode::Bridge));
    }

    #[tokio::test]
    async fn test_list_containers_only_running() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()]
        );
        let container2 = Container::new(
            "alpine:latest".to_string(),
            vec!["echo".to_string()]
        );
        runtime.create_container(container).await.unwrap();
        runtime.create_container(container2).await.unwrap();
        let running = runtime.list_containers(false).await.unwrap();
        assert!(running.is_empty());
        let all = runtime.list_containers(true).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_get_container_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let result = runtime.get_container("nonexistent-id").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CuboError::ContainerNotRunning(_)));
    }

    #[tokio::test]
    async fn test_remove_container_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let result = runtime.remove_container("nonexistent-id", false).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CuboError::ContainerNotRunning(_)));
    }

    #[tokio::test]
    async fn test_start_container_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let result = runtime.start_container("nonexistent-id", false).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CuboError::ContainerNotFound(_)));
    }

    #[tokio::test]
    async fn test_stop_container_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let result = runtime.stop_container("nonexistent-id", None).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CuboError::ContainerNotRunning(_)));
    }

    #[tokio::test]
    async fn test_create_container_with_name() {
        let temp = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()]
        ).with_name("my-test-container".to_string());
        let container_id = runtime.create_container(container).await.unwrap();
        let retrieved = runtime.get_container(&container_id).await.unwrap();
        assert_eq!(retrieved.name, Some("my-test-container".to_string()))
    }

    #[tokio::test]
    async fn test_multiple_containers() {
        let temp = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let c1 = Container::new("test:v1".to_string(), vec!["echo".to_string(), "1".to_string()]);
        let c2 = Container::new("test:v2".to_string(), vec!["echo".to_string(), "2".to_string()]);
        let c3 = Container::new("test:v3".to_string(), vec!["echo".to_string(), "3".to_string()]);

        let id1 = runtime.create_container(c1).await.unwrap();
        let id2 = runtime.create_container(c2).await.unwrap();
        let id3 = runtime.create_container(c3).await.unwrap();

        // All should be different
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        let all = runtime.list_containers(true).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_container_persistence_across_runtime() {
        let temp = TempDir::new().unwrap();
        let container_id: String;

        {
            let config = RuntimeConfig {
                root_dir: temp.path().to_path_buf(),
                ..Default::default()
            };
            let runtime = ContainerRuntime::new(config).unwrap();
            let container = Container::new(
                "persist:test".to_string(),
                vec!["echo".to_string(), "Hello World!!!".to_string()]
            ).with_name("persist-test".to_string());
            container_id = runtime.create_container(container).await.unwrap();
        }

        {
            let config = RuntimeConfig {
                root_dir: temp.path().to_path_buf(),
                ..Default::default()
            };
            let runtime = ContainerRuntime::new(config).unwrap();
            let containers = runtime.list_containers(true).await.unwrap();
            assert_eq!(containers.len(), 1);
            let loaded = runtime.get_container(&container_id).await.unwrap();
            assert_eq!(loaded.blueprint, "persist:test");
            assert_eq!(loaded.name, Some("persist-test".to_string()));
        }
    }

    #[test]
    fn test_execution_context_debug() {
        let container = Container::new("test:latest".to_string(), vec!["echo".to_string()]);
        let ctx = ExecutionContext {
            container,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            detach: false,
        };
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("ExecutionContext"));
        assert!(debug_str.contains("rootfs_path"));
    }

     #[test]
    fn test_execution_context_with_detach() {
        let container = Container::new("test:latest".to_string(), vec!["sleep".to_string(), "100".to_string()]);
        let ctx = ExecutionContext {
            container,
            rootfs_path: PathBuf::from("/var/run/container/rootfs"),
            detach: true,
        };
        assert!(ctx.detach);
        assert_eq!(ctx.rootfs_path, PathBuf::from("/var/run/container/rootfs"));
    }

    #[test]
    #[serial_test::serial]
    fn test_runtime_config_from_env_empty_value() {
        std::env::set_var("CUBO_ROOT", "");
        let cfg = RuntimeConfig::from_env();
        // Empty value should use default
        assert!(!cfg.root_dir.as_os_str().is_empty());
        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    #[serial_test::serial]
    fn test_default_root_dir_home_fallback() {
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", "/home/testuser");

        let dir = default_root_dir();
        assert_eq!(dir, PathBuf::from("/home/testuser/.local/state/cubo"));

        std::env::remove_var("HOME");
    }

    #[test]
    fn test_runtime_config_clone() {
        let config = RuntimeConfig {
            root_dir: PathBuf::from("/test/path"),
            default_network_mode: NetworkMode::Host,
            debug: true,
            container_timeout: 600,
        };
        let cloned = config.clone();
        assert_eq!(cloned.root_dir, PathBuf::from("/test/path"));
        assert!(matches!(cloned.default_network_mode, NetworkMode::Host));
        assert!(cloned.debug);
        assert_eq!(cloned.container_timeout, 600);
    }

    #[test]
    fn test_runtime_config_debug_trait() {
        let config = RuntimeConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("RuntimeConfig"));
        assert!(debug_str.contains("debug"));
    }

    #[tokio::test]
    async fn test_container_with_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["printenv".to_string()],
        )
        .with_env("FOO".to_string(), "bar".to_string())
        .with_env("BAZ".to_string(), "qux".to_string());

        let container_id = runtime.create_container(container).await.unwrap();
        let retrieved = runtime.get_container(&container_id).await.unwrap();

        assert_eq!(retrieved.config.env_vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(retrieved.config.env_vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[tokio::test]
    async fn test_container_with_workdir() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["pwd".to_string()],
        ).with_workdir("/app".to_string());

        let container_id = runtime.create_container(container).await.unwrap();
        let retrieved = runtime.get_container(&container_id).await.unwrap();

        assert_eq!(retrieved.config.working_dir, Some("/app".to_string()));
    }

    #[tokio::test]
    async fn test_container_with_volumes() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let volume = VolumeMount {
            host_path: "/tmp/data".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
            mount_type: MountType::Bind,
        };
        let container = Container::new(
            "test:latest".to_string(),
            vec!["ls".to_string()],
        ).with_volume(volume);

        let container_id = runtime.create_container(container).await.unwrap();
        let retrieved = runtime.get_container(&container_id).await.unwrap();

        assert_eq!(retrieved.config.volume_mounts.len(), 1);
        assert_eq!(retrieved.config.volume_mounts[0].container_path, "/data");
    }

    #[test]
    fn test_mount_volume_bind_directory() {
        let temp_dir = TempDir::new().unwrap();
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let host_dir = temp_dir.path().join("host");
        fs::create_dir_all(&host_dir).unwrap();

        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let volume = VolumeMount {
            host_path: host_dir.to_string_lossy().to_string(),
            container_path: "/data".to_string(),
            read_only: false,
            mount_type: MountType::Bind,
        };

        let result = runtime.mount_volume(&rootfs, &volume);
        assert!(result.is_ok());
        assert!(rootfs.join("data").exists());
    }

    #[test]
    fn test_mount_volume_bind_file() {
        let temp_dir = TempDir::new().unwrap();
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let host_file = temp_dir.path().join("config.json");
        fs::write(&host_file, "{}").unwrap();

        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let volume = VolumeMount {
            host_path: host_file.to_string_lossy().to_string(),
            container_path: "/etc/config.json".to_string(),
            read_only: true,
            mount_type: MountType::Bind,
        };

        let result = runtime.mount_volume(&rootfs, &volume);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_volume_tmpfs() {
        let temp_dir = TempDir::new().unwrap();
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let volume = VolumeMount {
            host_path: String::new(),
            container_path: "/tmp".to_string(),
            read_only: false,
            mount_type: MountType::Tmpfs,
        };

        let result = runtime.mount_volume(&rootfs, &volume);
        assert!(result.is_ok());
        assert!(rootfs.join("tmp").exists());
    }

    #[test]
    fn test_mount_volume_named_volume() {
        let temp_dir = TempDir::new().unwrap();
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let volume = VolumeMount {
            host_path: "my-volume".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
            mount_type: MountType::Volume,
        };

        let result = runtime.mount_volume(&rootfs, &volume);
        assert!(result.is_ok());
        assert!(rootfs.join("data").exists());
    }

    #[test]
    fn test_mount_volume_nonexistent_host() {
        let temp_dir = TempDir::new().unwrap();
        let rootfs = temp_dir.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();

        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let volume = VolumeMount {
            host_path: "/nonexistent/path".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
            mount_type: MountType::Bind,
        };

        let result = runtime.mount_volume(&rootfs, &volume);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_runtime_clone() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new("test:latest".to_string(), vec!["echo".to_string()]);
        runtime.create_container(container).await.unwrap();

        let cloned = runtime.clone();

        let original_list = runtime.list_containers(true).await.unwrap();
        let cloned_list = cloned.list_containers(true).await.unwrap();

        assert_eq!(original_list.len(), cloned_list.len());
    }

    #[tokio::test]
    async fn test_stop_container_already_stopped() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new("test:latest".to_string(), vec!["echo".to_string()]);
        let container_id = runtime.create_container(container).await.unwrap();

        let result = runtime.stop_container(&container_id, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_container_with_force() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new("test:latest".to_string(), vec!["echo".to_string()]);
        let container_id = runtime.create_container(container).await.unwrap();

        let result = runtime.remove_container(&container_id, true).await;
        assert!(result.is_ok());

        assert!(runtime.get_container(&container_id).await.is_err());
    }

    #[test]
    #[ignore] // Requires specific privileges; run manually with --ignored
    fn test_setup_user_uid_only() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        // This test is ignored by default because behavior depends on privileges:
        // - As root: changing uid will succeed
        // - As non-root: changing uid will fail
        let result = runtime.setup_user("1000");
        let _ = result;
    }

    #[test]
    #[ignore]
    fn test_setup_user_uid_gid() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let result = runtime.setup_user("1000:1000");
        let _ = result;
    }

    #[test]
    fn test_setup_user_invalid_uid() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let result = runtime.setup_user("notanumber");
        assert!(result.is_err());
    }

    #[test]
    fn test_setup_user_invalid_gid() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let result = runtime.setup_user("1000:notanumber");
        assert!(result.is_err());
    }

    #[test]
    fn test_setup_user_too_many_parts() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();

        let result = runtime.setup_user("1000:1000:extra");
        assert!(result.is_err());
    }
}
