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
