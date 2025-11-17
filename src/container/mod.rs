pub mod namespace;
pub mod runtime;
pub mod container_store;
pub mod image_store;
pub mod rootfs;
pub mod cubofile;
pub mod cubofile_toml;
pub mod builder;
pub mod registry;

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;





#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    /// Container ID
    pub id: String,
    /// Readable name
    pub name: Option<String>,
    /// Blueprint this container was created from
    pub blueprint: String,
    /// Command that runs this container
    pub command: Vec<String>,
    /// Status of the container
    pub status: ContainerStatus,
    /// Container Config
    pub config: ContainerConfig,
    /// When the container was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the container was started
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the container stopped running
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Exit code of the main process
    pub exit_code: Option<i32>,
    /// PID of the main container process
    pub pid: Option<u32>,

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Working directory inside the container
    pub working_dir: Option<String>,
    /// Environment variables
    pub env_vars: HashMap<String, String>,
    /// Volume mounts (host_path->container_path)
    pub volume_mounts: Vec<VolumeMount>,
    /// Port mappings (host_port->container_port)
    pub ports: Vec<PortMapping>,
    // Memoty limit in bytes
    pub memory_limit: Option<u64>,
    // CPU limit (number of cores, can be fractional)
    pub cpu_limit: Option<f32>,
    // User to run as (uid:gid)
    pub user: Option<String>,
    // Hostname in the containerdsadsadwq
    pub hostname: Option<String>,
    // Whether to allocate TTY
    pub tty: bool,
    // Where to keep the STDIN open
    pub stdin: bool,
    // Network Mode (bridge, host, none)
    pub network_mode: NetworkMode,
    // Restart policy
    pub restart_policy: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestartPolicy {
    No,
    Always,
    UnlessStopped,
    OnFailure { max_retries: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMode {
    // Default bridge network
    Bridge,
    // User host network stack
    Host,
    // No networking
    None,
    // Custom Network (Not sure about this one for now)
    Custom(String), 
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    /// Port on the host
    pub host_port: u16,
    /// Port in the container
    pub container_port: u16,
    /// Protocol (tcp/udp)
    pub protocol: Protocol,
    // Host IP to bind to
    pub host_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerStatus {
    Created,
    Running,
    Stopped,
    Paused,
    Error,
    Restarting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Path to the host directory to mount
    pub host_path: String,
    /// Path inside the container to mount the volume
    pub container_path: String,
    /// Wherher to mount as read-only
    pub read_only: bool,
    /// Mount type (bind, volume, tmpfs)
    pub mount_type: MountType, 
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MountType {
    /// Bind mount from host
    Bind,
    /// Name volume managed by the container runtime
    Volume,
    /// Temporary file system
    Tmpfs
}

impl Container {
    pub fn new(blueprint: String, command: Vec<String>) -> Self {
        Self {
            id: Self::generate_id(),
            name: None,
            blueprint,
            command,
            status: ContainerStatus::Created,
            config: ContainerConfig::default(),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: None,
            exit_code: None,
            pid: None,
        }
    }

    // Generate a unique container ID
    pub fn generate_id() -> String {
        Uuid::new_v4().to_string()
    }

    // Get short ID (first 12 characters of the ID )
    pub fn short_id(&self) -> String {
        self.id.chars().take(12).collect()
    }

    // Set container name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    // Set working directory
    pub fn with_workdir(mut self, workdir: String) -> Self {
        self.config.working_dir = Some(workdir);
        self
    }

    // Set environment variables
    pub fn with_env(mut self, name: String, value: String) -> Self {
        self.config.env_vars.insert(name, value);
        self
    }

    // Add volume mount
    pub fn with_volume(mut self, volume: VolumeMount) -> Self {
        self.config.volume_mounts.push(volume);
        self
    }

    // Add port mapping
    pub fn with_port(mut self, port: PortMapping) -> Self {
        self.config.ports.push(port);
        self
    }

    // Set memory limit
    pub fn with_memory_limit(mut self, limit: u64) -> Self {
        self.config.memory_limit = Some(limit);
        self
    }

    // Set CPU Limit
    pub fn with_cpu_limit(mut self, limit: f32) -> Self {
        self.config.cpu_limit = Some(limit);
        self
    }

    // Check if container is running
    pub fn is_running(&self) -> bool {
        matches!(self.status, ContainerStatus::Running)
    }

    pub fn is_stoppec(&self) -> bool {
        matches!(self.status, ContainerStatus::Stopped | ContainerStatus::Error)
    }

    pub fn update_status(&mut self, status: ContainerStatus) {
        self.status = status;
        match &self.status {
            ContainerStatus::Running => {
                if self.started_at.is_none() {
                    self.started_at = Some(chrono::Utc::now())
                }
            }
            ContainerStatus::Stopped | ContainerStatus::Error => {
                if self.finished_at.is_none() {
                    self.finished_at = Some(chrono::Utc::now())
                }

            }
            _ => {}
        }
    }

    // Set process ID PID
    pub fn set_pid(&mut self, pid: u32) {
        self.pid = Some(pid);
    }

    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = Some(code);
    }
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            env_vars: HashMap::new(),
            volume_mounts: Vec::new(),
            ports: Vec::new(),
            memory_limit: None,
            cpu_limit: None,
            user: None,
            hostname: None,
            tty: false,
            stdin: false,
            network_mode: NetworkMode::Bridge,
            restart_policy: RestartPolicy::No,
        }
    }
}

impl VolumeMount {
    pub fn bind(host_path: String, container_path: String, read_only: bool) -> Self {
        Self {
            host_path,
            container_path,
            read_only,
            mount_type: MountType::Bind,
        }
    }

    pub fn volume(volume_name: String, container_path: String, read_only: bool) -> Self {
        Self {
            host_path: volume_name,
            container_path,
            read_only,
            mount_type: MountType::Volume,
        }
    }

    pub fn tmpfs(container_path: String) -> Self {
        Self {
            host_path: String::new(),
            container_path,
            read_only: false,
            mount_type: MountType::Tmpfs,
        }
    }
}

impl PortMapping {
    pub fn tcp(host_port: u16, container_port: u16) -> Self {
        Self {
            host_port,
            container_port,
            protocol: Protocol::Tcp,
            host_ip: None,
        }
    }

    pub fn udp(host_port: u16, container_port: u16) -> Self {
        Self {
            host_port,
            container_port,
            protocol: Protocol::Udp,
            host_ip: None,
        }
    }

    pub fn with_host_ip(mut self, ip: String) -> Self {
        self.host_ip = Some(ip);
        self
    }
}

impl std::fmt::Display for ContainerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerStatus::Created => write!(f, "Created"),
            ContainerStatus::Running => write!(f, "Running"),
            ContainerStatus::Stopped => write!(f, "Stopped"),
            ContainerStatus::Paused => write!(f, "Paused"),
            ContainerStatus::Error => write!(f, "Error"),
            ContainerStatus::Restarting => write!(f, "Restarting"),
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "tcp"),
            Protocol::Udp => write!(f, "udp"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_creation() {
        let container = Container::new(
            "ubuntu:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()]
        );

        assert_eq!(container.blueprint, "ubuntu:latest");
        assert_eq!(container.command, vec!["echo", "hello"]);
        assert_eq!(container.status, ContainerStatus::Created);
        assert!(container.name.is_none());
        assert!(container.pid.is_none());
    }

    #[test]
    fn test_container_builder_pattern() {
        let container = Container::new("ubuntu:latest".to_string(), vec!["bash".to_string()])
            .with_name("test-container".to_string())
            .with_workdir("/app".to_string())
            .with_env("HOME".to_string(), "/root".to_string())
            .with_memory_limit(1024 * 1024 * 1024); 

        assert_eq!(container.name, Some("test-container".to_string()));
        assert_eq!(container.config.working_dir, Some("/app".to_string()));
        assert_eq!(
            container.config.env_vars.get("HOME"),
            Some(&"/root".to_string())
        );
        assert_eq!(container.config.memory_limit, Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_volume_mount_creation() {
        let bind_mount = VolumeMount::bind(
            "/host/path".to_string(),
            "/container/path".to_string(),
            true
        );
        
        assert_eq!(bind_mount.host_path, "/host/path");
        assert_eq!(bind_mount.container_path, "/container/path");
        assert!(bind_mount.read_only);
        assert!(matches!(bind_mount.mount_type, MountType::Bind));
    }

    #[test]
    fn test_port_mapping_creation() {
        let port = PortMapping::tcp(8080, 80).with_host_ip("127.0.0.1".to_string());

        assert_eq!(port.host_port, 8080);
        assert_eq!(port.container_port, 80);
        assert!(matches!(port.protocol, Protocol::Tcp));
        assert_eq!(port.host_ip, Some("127.0.0.1".to_string()));
    }
}

