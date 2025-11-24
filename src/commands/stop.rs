use crate::cli::StopArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::container::Container;
use crate::error::Result;
use tracing::{info, warn, error};

pub async fn execute(args: StopArgs) -> Result<()> {
    if args.containers.is_empty() {
        error!("No contiainers specified");
        return Err(crate::error::CuboError::InvalidConfiguration(
            "At least one container must be specified".to_string()
        ));
    }

    info!("Removing {} container(s)", args.containers.len());

    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config)?;

    let mut removed_containers: Vec<String> = Vec::new();
    let mut failed_containers: Vec<(String, crate::error::CuboError)> = Vec::new();

    for container_identifier in args.containers {
        match remove_single_container(&runtime, &container_identifier, args.force).await {
            Ok(_container_id) => {
                removed_containers.push(container_identifier.clone());
                info!("Removed container: {}", container_identifier);
                println!("{}", container_identifier);
            }
            Err(e) => {
                error!("Failed to remove container {}: {}", container_identifier, e);
                failed_containers.push((container_identifier.clone(), e));
            }
        }
    }

    if !failed_containers.is_empty() {
        warn!("Failed to remove {} container(s)", failed_containers.len());
        for (container, error) in failed_containers {
            eprintln!("Error removing {}: {}", container, error);
        }

        return Err(crate::error::CuboError::SystemError(
            "Some containers could not be removed".to_string()
        ));
    }

    info!("Successfully removed {} container(s)", removed_containers.len());
    Ok(())
}

async fn remove_single_container(
    runtime: &ContainerRuntime,
    identifier: &str,
    force: bool
) -> Result<String> {
    let container_id = find_container_id(runtime, identifier).await?;

    runtime.remove_container(&container_id, force).await?;

    Ok(container_id)
}

async fn find_container_id(runtime: &ContainerRuntime, identifier: &str) -> Result<String> {
    let containers: Vec<Container> = runtime.list_containers(true).await?;

    for container in &containers {
        if container.id == identifier {
            return Ok(container.id.clone());
        }
    }

    for container in &containers {
        if container.id.starts_with(identifier) {
            return Ok(container.id.clone())
        }
    }

    for container in &containers {
        if let Some(ref name) = container.name {
            if name == identifier {
                return Ok(container.id.clone());
            }
        }
    }

    Err(crate::error::CuboError::ContainerNotFound(identifier.to_string()))

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Container;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_empty_containers() {
        let args = StopArgs {
            containers: vec![],
            force: false,
        };

        let result = execute(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("At least one container"));
    }

    #[tokio::test]
    async fn test_execute_nonexistant_container() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());

        let args = StopArgs {
            containers: vec!["nonexistent".to_string()],
            force: false,
        };
        let result = execute(args).await;
        assert!(result.is_err());
        std::env::remove_var("CUBO_ROOT");
    }

    #[tokio::test]
    async fn test_find_container_id_exact_match() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime  = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()],
        ).with_name("stop-test".to_string());
        let container_id = runtime.create_container(container).await.unwrap();

        let result = find_container_id(&runtime, &container_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), container_id);
    }

    #[tokio::test]
    async fn test_find_container_id_partial_match() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()],
        );

        let container_id = runtime.create_container(container).await.unwrap();
        let partial_id = &container_id[..8];
        let result = find_container_id(&runtime, partial_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), container_id);
    }

    #[tokio::test]
    async fn test_container_id_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()],
        ).with_name("my-named-container".to_string());
        let container_id = runtime.create_container(container).await.unwrap();
        let result = find_container_id(&runtime, "my-named-container").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), container_id);
    }

    #[tokio::test]
    async fn test_find_container_id_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let result = find_container_id(&runtime, "nonexistent").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, crate::error::CuboError::ContainerNotFound(_)));
    }

    #[tokio::test]
    async fn test_remove_single_container_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string()]
        ).with_name("remove_test".to_string());
        let container_id = runtime.create_container(container).await.unwrap();
        let result = remove_single_container(&runtime, "remove_test", false).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), container_id);
        assert!(runtime.get_container(&container_id).await.is_err());
    }
}
