use std::collections::HashMap as Map;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::container::{Container, ContainerStatus};
use crate::error::{CuboError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciState {
    #[serde(rename = "ociVersion")]
    pub oci_version: String,
    pub id: String,
    pub status: String,
    pub pid: Option<u32>,
    pub bundle: String,
    pub annotations: Map<String, String>,
}

impl OciState {
    pub fn new(container: &Container, bundle: &Path) -> Self {
        let (status, error_flag) = oci_status_from_container(&container.status);
        let mut annotations: HashMap<String, String> = Map::new();
        if let Some(name) = &container.name {
            annotations.insert("name".into(), name.clone());
        }
        annotations.insert("blueprint".into(), container.blueprint.clone());
        if error_flag {
            annotations.insert("error".into(), "true". into());
        }
        Self {
            oci_version: "1.0.2".into(),
            id: container.id.clone(),
            status,
            pid: container.pid,
            bundle: bundle.to_string_lossy().to_string(),
            annotations,
        }
    }
}

fn oci_status_from_container(status: &ContainerStatus) -> (String, bool) {
    match status {
        ContainerStatus::Created => ("created".into(), false),
        ContainerStatus::Running => ("running".into(), false),
        ContainerStatus::Stopped => ("stopped".into(), false),
        ContainerStatus::Paused => ("paused".into(), false),
        ContainerStatus::Error => ("unknown".into(), true),
        ContainerStatus::Restarting => ("unknown".into(), false),
    }
}

fn container_status_from_oci(s: &str) -> Option<ContainerStatus> {
    match s {
        "created" => Some(ContainerStatus::Created),
        "running" => Some(ContainerStatus::Running),
        "stopped" => Some(ContainerStatus::Stopped),
        "paused" => Some(ContainerStatus::Paused),
        _ => None,
    }
}

pub fn save_config(root_dir: &Path, container: &Container) -> Result<()> {
    let bundle_dir: PathBuf = root_dir.join(&container.id);
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| CuboError::SystemError(format!("Failed to create bundle dir: {}", e)))?;
    let cfg_path = bundle_dir.join("config.json");
    atomic_write_json(&cfg_path, container)
}

pub fn save_state(root_dir: &Path, container: &Container) -> Result<()> {
    let bundle_dir: PathBuf = root_dir.join(&container.id);
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| CuboError::SystemError(format!("Failed to create bundle dir: {}", e)))?;
    let st_path = bundle_dir.join("state.json");
    let state = OciState::new(container, &bundle_dir);

    atomic_write_json(&st_path, &state)
}
 
pub fn load_all(root_dir: &Path) -> Result<HashMap<String, Container>> {
    let mut loaded: HashMap<String, Container> = HashMap::new();
    if !root_dir.exists() {
        return Ok(loaded);
    }
    
    for entry in fs::read_dir(root_dir)
        .map_err(|e| CuboError::SystemError(format!("Failed to read root dir: {}", e)))?
        {
            let entry = entry.map_err(|e| CuboError::SystemError(format!("Failed to read dir entry: {}", e)))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let config_path = path.join("config.json");
            if !config_path.exists() {
                continue;
            }
            let mut container: Container = read_json(&config_path)?;
            let state_path = path.join("state.json");
            if state_path.exists() {
                if let Ok(state) = read_json::<OciState>(&state_path) {
                    if let Some(s) = container_status_from_oci(&state.status) {
                        container.update_status(s);
                    } 
                    container.pid = state.pid;
                }
            }
            loaded.insert(container.id.clone(), container);
        }
        Ok(loaded)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T>{
    let data = fs::read_to_string(path)
        .map_err(|e| CuboError::SystemError(format!("Failed to read {}: {}", path.display(), e)))?;
    let value = serde_json::from_str(&data)
        .map_err(|e| CuboError::SystemError(format!("Failed to parse JSON from {}: {}", path.display(), e)))?;
    Ok(value)
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        CuboError::SystemError(format!("No parent directory for {}", path.display()))
    })?;

    fs::create_dir_all(parent)
        .map_err(|e| CuboError::SystemError(format!("Failed to create parent dir: {}", e)))?;

    let tmp_path = tmp_path_for(path);
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| CuboError::SystemError(format!("Failed to serialize JSON: {}", e)))?;

    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| CuboError::SystemError(format!("Failed to create tmp File: {}", e)))?;
        f.write_all(json.as_bytes())
            .map_err(|e| CuboError::SystemError(format!("Failed to write tmp file: {}", e)))?;
        f.sync_all()
            .map_err(|e| CuboError::SystemError(format!("Failed to sync tmp file: {}", e)))?;
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        CuboError::SystemError(format!(
            "Failed to rename tmp file to target {} -> {}: {}", 
            tmp_path.display(),
            path.display(),
            e
        ))
    })?;

    Ok(())
}

fn tmp_path_for(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tmp.json")
        .to_string();
    name.push_str(".tmp");
    target.parent().unwrap_or_else(|| Path::new(".")).join(name)
}

/// PID Liveness check using libc::kill(pid, 0)
pub fn pid_is_alive(pid: Option<u32>) -> bool {
    let pid = match pid {
        Some(p) => p as libc::pid_t,
        None => return false,
    };
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }

    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => true,
        Some(libc::ESRCH) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn demo_container() -> Container {
        Container::new(
            "demo:latest".to_string(),
            vec!["/bin/echo".to_string(), "Hello world!!!".to_string()]
        )
    }

    #[test]
    fn test_oci_state_mapping_and_annotations() {
        let mut c = demo_container();
        c.update_status(ContainerStatus::Created);
        let st = OciState::new(&c, Path::new("/bundle/123"));
        assert_eq!(st.status, "created");
        assert_eq!(
            st.annotations.get("blueprint").cloned(),
            Some("demo:latest".into())
        );

        // Running
        c.update_status(ContainerStatus::Running);
        let st = OciState::new(&c, Path::new("/bundle/123"));
        assert_eq!(st.status, "running");

        // Paused
        c.update_status(ContainerStatus::Paused);
        let st = OciState::new(&c, Path::new("/bundle/123"));
        assert_eq!(st.status, "paused");

        // Stopped
        c.update_status(ContainerStatus::Stopped);
        let st = OciState::new(&c, Path::new("/bundle/123"));
        assert_eq!(st.status, "stopped");

        // Error -> unknown + error annotation
        c.update_status(ContainerStatus::Error);
        let st = OciState::new(&c, Path::new("/bundle/123"));
        assert_eq!(st.status, "unknown");
        assert_eq!(st.annotations.get("error").cloned(), Some("true".into()))
    }

    #[test]
    fn test_atomic_json_write_and_read() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("date.json");

        // first write
        atomic_write_json(&p, &serde_json::json!({"a": 1})).unwrap();
        let v: serde_json::Value = read_json(&p).unwrap();
        assert_eq!(v["a"], 1);

        // overwrite
        atomic_write_json(&p, &serde_json::json!({"a": 2, "b": 3})).unwrap();
        let v2: serde_json::Value = read_json(&p).unwrap();
        assert_eq!(v2["a"], 2);
        assert_eq!(v2["b"], 3);

        // ensure no linering tmp file
        let tempfile = p.parent().unwrap().join("data.json.tmp");
        assert!(!tempfile.exists());
    }

    #[test]
    fn test_save_config_and_state_and_load_all() {
        let tmp = TempDir::new().unwrap();
        let mut c = demo_container();

        c.name = Some("demo".into());
        c.set_pid(12345);
        c.update_status(ContainerStatus::Running);

        save_config(tmp.path(), &c).unwrap();
        save_state(tmp.path(), &c).unwrap();

        let bundle = tmp.path().join(&c.id);
        assert!(bundle.join("config.json").exists());
        assert!(bundle.join("state.json").exists());

        let loaded = load_all(tmp.path()).unwrap();
        let c2 = loaded.get(&c.id).unwrap();
        assert_eq!(c2.id, c.id);
        assert_eq!(c2.status, ContainerStatus::Running);
        assert_eq!(c2.pid, Some(12345));
    }


}