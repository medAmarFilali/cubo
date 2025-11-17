use crate::cli::RmArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::error::Result;
use tracing::{info, warn, error};


pub async fn execute(args: RmArgs) -> Result<()> {
    if args.containers.is_empty() {
        error!("No containers specified");
        return Err(crate::error::CuboError::InvalidConfiguration(
            "At least one container must be specified".to_string()
        ))
    }

    info!("Removing {} containers(s)", args.containers.len());

    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config)?;

    let  mut removed_containers = Vec::new();
    let mut failed_containers = Vec::new();

    for container_identifier in args.containers {
        match remove_single_container(&runtime, &container_identifier, args.force).await {
            Ok(_container_id) => {
                removed_containers.push(container_identifier.clone());
                info!("Removed container: {}", container_identifier);
                println!("{}", container_identifier);
            }
            Err(e) => {
                error!("Filed to remove container {}: {}", container_identifier, e);
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

    info!("Suvvessfully removed {} container(s)", removed_containers.len());


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

/// Find container ID by partial ID or name
async fn find_container_id(runtime: &ContainerRuntime, identifier: &str) -> Result<String> {
    let containers = runtime.list_containers(true).await?;
    
    // First, try exact ID match
    for container in &containers {
        if container.id == identifier {
            return Ok(container.id.clone());
        }
    }
    
    // Then try partial ID match (like Docker)
    for container in &containers {
        if container.id.starts_with(identifier) {
            return Ok(container.id.clone());
        }
    }
    
    // Finally, try name match
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
    async fn test_remove_single_container() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        
        // Create a test container
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "test".to_string()],
        ).with_name("test-container".to_string());
        
        let container_id = runtime.create_container(container).await.unwrap();
        
        // Test removing by name
        let result = remove_single_container(&runtime, "test-container", false).await;
        assert!(result.is_ok());
        
        // Verify container is gone
        assert!(runtime.get_container(&container_id).await.is_err());
    }

    #[tokio::test]
    async fn test_find_container_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        
        // Create a test container
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "test".to_string()],
        ).with_name("test-container".to_string());
        
        let container_id = runtime.create_container(container).await.unwrap();
        
        // Test exact ID match
        assert_eq!(
            find_container_id(&runtime, &container_id).await.unwrap(),
            container_id
        );
        
        // Test partial ID match
        let partial_id = &container_id[..8];
        assert_eq!(
            find_container_id(&runtime, partial_id).await.unwrap(),
            container_id
        );
        
        // Test name match
        assert_eq!(
            find_container_id(&runtime, "test-container").await.unwrap(),
            container_id
        );
        
        // Test not found
        assert!(find_container_id(&runtime, "nonexistent").await.is_err());
    }
}