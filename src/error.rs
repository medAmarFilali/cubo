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