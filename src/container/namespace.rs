use std::ffi::CString;
use nix::sched::{unshare, CloneFlags};
use nix::unistd::{chdir, getegid, geteuid};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use crate::container::{NetworkMode};
use crate::error::{CuboError, Result};


#[derive(Debug, Clone, Copy)]
pub struct UnshareInfo {
    pub user: bool,
    pub mnt: bool,
    pub pid: bool,
    pub uts: bool, 
    pub net: bool,
}

/// Unshare into a new user namespace, then map container root (0) to current host uid/gid.
/// Writes /proc/self/setgroups (deny) before gid_map as required by the kernel.
pub fn unshare_user_then_map_ids() -> Result<()> {
    let uid = geteuid().as_raw();
    let gid = getegid().as_raw();

    if uid == 0 {
        tracing::debug!("Running as root (uid=0), skipping user namespace creation");
        return Ok(());
    }
    unshare(CloneFlags::CLONE_NEWUSER)
        .map_err(|e| CuboError::NamespaceError(format!("Failed to clone user: {}", e)))?;

    match fs::write("/proc/self/setgroups", b"deny") {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound || e.kind() == ErrorKind::InvalidInput => {}
        Err(e) => {
            return Err(CuboError::NamespaceError(format!(
                "Failed to write /proc/self/setgroups: {}",
                e
            )))
        }
    }

    fs::write("/proc/self/uid_map", format!("0 {} 1\n", uid))
        .map_err(|e| CuboError::NamespaceError(format!("Failed to write uid_map: {}", e)))?;

    fs::write("/proc/self/gid_map", format!("0 {} 1\n", gid))
        .map_err(|e| CuboError::NamespaceError(format!("Failed to write gid_map: {}", e)))?;

    Ok(())
}


/// unshare mount, pid, uts, and optionally net namespaces depeding on the networkmode (host)
pub fn unshare_mount_pid_net(mode: &NetworkMode) -> Result<UnshareInfo> {
    let mut flags = CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWUTS;
    let mut net = false;

    if !matches!(mode, NetworkMode::Host) {
        flags |= CloneFlags::CLONE_NEWNET;
        net = true;
    }

    unshare(flags)
        .map_err(|e| CuboError::NamespaceError(format!("unshare(mnt, pid, uts, net) failed: {}", e)))?;

    Ok(UnshareInfo {user:true, mnt: true, pid: true, uts: true, net})
}

/// Remount the root with privcate propagation to avoid mount leaks back to host.
pub fn make_mounts_private() -> Result<()> {
    mount::<str, std::path::Path, str, str>(
        None,
        Path::new("/"),
        None,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>
    )
    .map_err(|e| CuboError::NamespaceError(format!("Failed to make mounts private: {}", e)))?;

    Ok(())
}


/// Bind-mount a host path onto the target. Optionally remount read-only.
pub fn bind_mount(host: &Path, target: &Path, read_only:bool) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(|e| CuboError::VolumeError(format!(
                "Failed to create mount target parent {:?}: {}", 
                parent, e
            )))?;
    }

    if host.is_dir() {
        if !target.exists() {
            fs::create_dir_all(target)
                .map_err(|e| CuboError::VolumeError(format!("Failed to create dir {:?}: {}", target, e)))?;
        }
    } else if host.is_file() && !target.exists() {
        // Create an empty file as the mount point
        fs::File::create(target)
            .map_err(|e| CuboError::VolumeError(format!("Failed to create file {:?}: {}", target, e)))?;
    }

    mount::<std::path::Path, std::path::Path, str, str>(
        Some(host),
        target,
        None,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| CuboError::VolumeError(format!(
        "Failed to bind-mount {:?} -> {:?}: {}",
        host, target, e
    )))?;

        if read_only {
        mount::<std::path::Path, std::path::Path, str, str>(
            Some(host),
            target,
            None,
            MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY,
            None::<&str>,
        )
        .map_err(|e| CuboError::VolumeError(format!(
            "Failed to remount read-only {:?}: {}",
            target, e
        )))?;
    }


    Ok(())
}

pub fn pivot_to_rootfs(rootfs: &Path) -> Result<()> {
    mount::<std::path::Path, std::path::Path, str, str>(
        Some(rootfs),
        rootfs,
        None,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>
    )
    .map_err(|e| CuboError::NamespaceError(format!("Bind-mount rootfs failed: {}", e)))?;

    chdir(rootfs).map_err(|e| CuboError::NamespaceError(format!("chrdir(rootfs) failed: {}", e)))?;

    // Create put_old directory
    let oldroot: &Path = Path::new("oldroot");
    if !oldroot.exists() {
        fs::create_dir_all(oldroot)
            .map_err(|e| CuboError::NamespaceError(format!("mkdir oldroot failed: {}", e)))?;
    }

    let new_root_c = CString::new(".").unwrap();
    let put_old_c = CString::new("oldroot").unwrap();
    let rc = unsafe {libc::syscall(libc::SYS_pivot_root, new_root_c.as_ptr(), put_old_c.as_ptr()) };
    if rc != 0 {
        return Err(CuboError::NamespaceError(format!(
            "pivot_root failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    // Now we're in the new root; compelte the switch
    chdir("/").map_err(|e| CuboError::NamespaceError(format!("chdir(/) failed: {}", e)))?;
    umount2("/oldroot", MntFlags::MNT_DETACH)
        .map_err(|e| CuboError::NamespaceError(format!("umount /oldroot failed: {}", e)))?;

    let _ = fs::remove_dir_all("/oldroot");

    Ok(())
}

/// Mount proc inside the current root
pub fn mount_proc() -> Result<()> {
    // Ensure /proc exists
    if !Path::new("/proc").exists() {
        fs::create_dir_all("/proc")
            .map_err(|e| CuboError::NamespaceError(format!("mkdir /proc failed: {}", e)))?;
    }
    mount::<str, str, str, str,>(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None,
    )
    .map_err(|e| CuboError::NamespaceError(format!("Mount proc failed: {}", e)))?;

    Ok(())
}

pub fn setup_loopback() -> Result<()> {
    let try_ip = std::process::Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .status();

    if let Ok(status) = try_ip {
        if status.success() {
            return Ok(());
        }
    }

    let _ = std::process::Command::new("ifconfig")
        .args(["lo", "up"])
        .status();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_unshare_info_struct() {
        let info = UnshareInfo {
            user: true,
            mnt: true,
            pid: true,
            uts: true,
            net: false,
        };
        assert!(info.user);
        assert!(info.mnt);
        assert!(info.pid);
        assert!(info.uts);
        assert!(!info.net);
    }

    #[test]
    fn test_unshare_info_debug() {
        let info = UnshareInfo {
            user: true,
            mnt: false,
            pid: true,
            uts: false,
            net: true,
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("UnshareInfo"));
        assert!(debug_str.contains("user: true"));
        assert!(debug_str.contains("mnt: false"));
    }

    #[test]
    fn test_unshare_info_clone() {
        let info = UnshareInfo {
            user: true,
            mnt: true,
            pid: false,
            uts: true,
            net: false
        };
        let cloned = info;
        assert_eq!(cloned.user, info.user);
        assert_eq!(cloned.mnt, info.mnt);
        assert_eq!(cloned.pid, info.pid);
        assert_eq!(cloned.uts, info.uts);
        assert_eq!(cloned.net, info.net);
    }

    #[test]
    fn test_bind_mount_creates_target_parent_dirs() {
        let temp = TempDir::new().unwrap();
        let host_dir = temp.path().join("host_dir");
        let target = temp.path().join("deep/nested/target");
        fs::create_dir_all(&host_dir).unwrap();
        let _result = bind_mount(&host_dir, &target, false);
        // Parent dirs should be created regardless of mount success/failure
        assert!(target.parent().unwrap().exists());
    }

    #[test]
    fn test_bind_mount_creates_dir_target_for_dir_host() {
        let temp = TempDir::new().unwrap();
        let host_dir = temp.path().join("host_dir");
        let target = temp.path().join("target_dir");
        fs::create_dir_all(&host_dir).unwrap();
        let _result = bind_mount(&host_dir, &target, false);
        assert!(target.exists());
        assert!(target.is_dir());
    }

    #[test]
    fn test_bind_mount_creates_dir_target_for_file_host() {
        let temp = TempDir::new().unwrap();
        let host_file = temp.path().join("host_file");
        let target = temp.path().join("target_file");
        fs::write(&host_file, "content").unwrap();
        let _result = bind_mount(&host_file, &target, false);
        assert!(target.exists());
        assert!(target.is_file());
    }

    #[test]
    #[ignore]
    fn test_unshare_user_then_map_ids_as_non_root() {
        let result = unshare_user_then_map_ids();
        println!("unshare_user result: {:?}", result);
    }

    #[test]
    #[ignore]
    fn test_unshare_mount_pid_net_bridge_mode() {
        let result = unshare_mount_pid_net(&NetworkMode::Bridge);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert!(info.mnt);
        assert!(info.pid);
        assert!(info.uts);
        assert!(info.net);
    }

    #[test]
    #[ignore]
    fn test_unshare_mount_pid_net_host_mode() {
        let result = unshare_mount_pid_net(&NetworkMode::Host);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert!(info.mnt);
        assert!(info.pid);
        assert!(info.uts);
        // Host mode doesn't create net namespace
        assert!(!info.net);
    }

    #[test]
    #[ignore] // requires root previleges
    fn test_make_mounts_private() {
        let result = make_mounts_private();
        println!("make_mounts_private result: {:?}", result);
    }

    #[test]
    #[ignore]
    fn test_pivot_to_rootfs() {
        let temp = TempDir::new().unwrap();
        let rootfs = temp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let result = pivot_to_rootfs(&rootfs);
        println!("pivot_to_rootfs result: {:?}", result);
    }

    #[test]
    #[ignore]
    fn test_mount_proc() {
        let result = mount_proc();
        println!("mount_proc result: {:?}", result);
    }

    #[test]
    fn test_setup_loopback_best_effort() {
        let result = setup_loopback();
        assert!(result.is_ok());
    }
}