use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum CuboError {
    #[error("Container not found: {0}")]
    ContainerNotFound(String),

    #[error("Blueprint not found: {0}")]
    BlueprintNotFound(String),

    #[error("Container already exists: {0}")]
    ContainerAlreadyExists(String),

    #[error("Container is not running: {0}")]
    ContainerNotRunning(String),

    #[error("Container is already running: {0}")]
    ContainerAlreadyRunning(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("System error: {0}")]
    SystemError(String),

    #[error("Volume error: {0}")]
    VolumeError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Namespace error: {0}")]
    NamespaceError(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("UUID error:v {0}")]
    UuidError(#[from] uuid::Error),
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, CuboError>;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_not_found_display() {
        let err = CuboError::ContainerNotFound("test-container".to_string());
        assert_eq!(err.to_string(), "Container not found: test-container");
    }

    #[test]
    fn test_blueprint_not_found_display() {
        let err = CuboError::BlueprintNotFound("alpine:latest".to_string());
        assert_eq!(err.to_string(), "Blueprint not found: alpine:latest");
    }

    #[test]
    fn test_container_already_exists_display() {
        let err = CuboError::ContainerAlreadyExists("my-container".to_string());
        assert_eq!(err.to_string(), "Container already exists: my-container");
    }

    #[test]
    fn test_container_already_running_display() {
        let err = CuboError::ContainerAlreadyRunning("running-container".to_string());
        assert_eq!(err.to_string(), "Container is already running: running-container");
    }

    #[test]
    fn test_permission_denied_display() {
        let err = CuboError::PermissionDenied("cannot access /root".to_string());
        assert_eq!(err.to_string(), "Permission denied: cannot access /root");
    }

    #[test]
    fn test_invalid_config_display() {
        let err = CuboError::InvalidConfiguration("missing base image".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: missing base image");
    }

    #[test]
    fn test_system_error_display() {
        let err = CuboError::SystemError("fork failed".to_string());
        assert_eq!(err.to_string(), "System error: fork failed");
    }

    #[test]
    fn test_network_error_display() {
        let err = CuboError::NetworkError("connection refused".to_string());
        assert_eq!(err.to_string(), "Network error: connection refused");
    }

    #[test]
    fn test_volume_error_display() {
        let err = CuboError::VolumeError("mount failed".to_string());
        assert_eq!(err.to_string(), "Volume error: mount failed");
    }

    #[test]
    fn test_namespace_error_displau() {
        let err = CuboError::NamespaceError("unshare failed".to_string());
        assert_eq!(err.to_string(), "Namespace error: unshare failed");
    }

    #[test]
    fn test_process_error_display() {
        let err = CuboError::ProcessError("exec failed".to_string());
        assert_eq!(err.to_string(), "Process error: exec failed");
    }

    #[test]
    fn test_io_error_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let cubo_err: CuboError = io_err.into();
        assert!(matches!(cubo_err, CuboError::IoError(_)));
        assert!(cubo_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_error_debut_impl() {
        let err = CuboError::ContainerNotFound("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ContainerNotFound"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }
        fn returns_err() -> Result<i32> {
            Err(CuboError::SystemError("test".to_string()))
        }
        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }


}