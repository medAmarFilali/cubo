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
    unshare(CloneFlags::CLONE_NEWUSER)
        .map_err(|e| CuboError::NamespaceError(format!("Failed to clone user: {}", e)))?;

    let uid = geteuid().as_raw();
    let gid = getegid().as_raw();

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