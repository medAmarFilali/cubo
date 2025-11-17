use crate::cli::RunArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::container::{Container, VolumeMount, PortMapping, Protocol};
use crate::error::Result;
use tracing::{info, warn, error};

pub async fn execute(args: RunArgs) -> Result<()> {
    info!("Running container with blueprint: {}", args.blueprint);

    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config)?;

    let command = if let Some(cmd) = args.command {
        cmd
    } else {
        vec!["/bin/sh".to_string()]
    };

    let mut container = Container::new(args.blueprint.clone(), command);

    if let Some(name) = args.name {
        container = container.with_name(name);
    }

    if let Some(workdir) = args.workdir {
        container = container.with_workdir(workdir);
    }

    for env_var in args.env {
        if let Some((key, value)) = parse_env_var(&env_var) {
            container = container.with_env(key, value);
        } else {
            warn!("Invalid environment variable format: {}", env_var);
        }
    }

    for volume in args.volume {
        if let Some(volume_mount) = parse_volume(&volume) {
            container = container.with_volume(volume_mount);
        } else {
            warn!("Invalid volume format: {}", volume);
        }
    }

    for port in args.publish {
        if let Some(port_mapping) = parse_port(&port) {
            container = container.with_port(port_mapping);
        } else {
            warn!("Invalid port format: {}", port);
        }
    }

    let container_id = runtime.create_container(container).await?;
    info!("Created container: {}", container_id);

    info!("Starting container: {}", container_id);

    match runtime.start_container(&container_id, args.detach).await {
        Ok(_) => {
            if args.detach {
                println!("{}", container_id);
                info!("Container started in detached mode");
            } else {
                match runtime.get_container(&container_id).await {
                    Ok(container) => {
                        info!("Container finished with status: {}", container.status);
                        if let Some(exit_code) = container.exit_code {
                            info!("Exit code: {}", exit_code);
                            std::process::exit(exit_code);
                        }
                    }
                    Err(e) => error!("Failed to get final container status: {}", e),
                }
            }
        }
        Err(e) => {
            error!("Failed to start container: {}", e);
            if let Err(cleanup_err) = runtime.remove_container(&container_id, true).await {
                error!("Failed to cleanup container after start failure: {}", cleanup_err);
            }
            return Err(e);
        }
    }

    Ok(())
}

fn parse_env_var(env_str: &str) -> Option<(String, String)> {
    if let Some((key, value)) = env_str.split_once('=') {
        Some((key.to_string(), value.to_string()))
    } else {
        None
    }
}

fn parse_volume(volume_str: &str) -> Option<VolumeMount> {
    let parts: Vec<&str> = volume_str.split(':').collect();

    match parts.len() {
        2 => {
            Some(VolumeMount::bind(
                parts[0].to_string(),
                parts[1].to_string(),
                false
            ))
        }
        3 => {
            let read_only = parts[2] == "ro";
            Some(VolumeMount::bind(
                parts[0].to_string(),
                parts[1].to_string(),
                read_only
            ))
        }
        _ => None,
    }
}

fn parse_port(port_str: &str) -> Option<PortMapping> {
    // Handle protocol suffix (e.g., "8080:80/tcp")
    let (port_part, protocol) = if let Some((ports, proto)) = port_str.split_once('/') {
        let protocol = match proto.to_lowercase().as_str() {
            "tcp" => Protocol::Tcp,
            "udp" => Protocol::Udp,
            _ => Protocol::Tcp, // default to TCP
        };
        (ports, protocol)
    } else {
        (port_str, Protocol::Tcp) // default to TCP
    };
    
    // Parse host:container ports
    if let Some((host_port_str, container_port_str)) = port_part.split_once(':') {
        if let (Ok(host_port), Ok(container_port)) = 
            (host_port_str.parse::<u16>(), container_port_str.parse::<u16>()) {
            Some(PortMapping {
                host_port,
                container_port,
                protocol,
                host_ip: None,
            })
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::MountType;

    #[test]
    fn test_parse_env_var() {
        assert_eq!(
            parse_env_var("HOME=/root"),
            Some(("HOME".to_string(), "/root".to_string()))
        );
        assert_eq!(
            parse_env_var("PATH=/usr/bin:/bin"),
            Some(("PATH".to_string(), "/usr/bin:/bin".to_string()))
        );
        assert_eq!(parse_env_var("INVALID"), None);
    }

    #[test]
    fn test_parse_volume() {
        let volume = parse_volume("/host/path:/container/path").unwrap();
        assert_eq!(volume.host_path, "/host/path");
        assert_eq!(volume.container_path, "/container/path");
        assert!(!volume.read_only);
        assert!(matches!(volume.mount_type, MountType::Bind));

        let ro_volume = parse_volume("/host/path:/container/path:ro").unwrap();
        assert!(ro_volume.read_only);

        assert!(parse_volume("invalid").is_none());
    }

    #[test]
    fn test_parse_port() {
        let port = parse_port("8080:80").unwrap();
        assert_eq!(port.host_port, 8080);
        assert_eq!(port.container_port, 80);
        assert!(matches!(port.protocol, Protocol::Tcp));

        let udp_port = parse_port("8080:80/udp").unwrap();
        assert!(matches!(udp_port.protocol, Protocol::Udp));

        assert!(parse_port("invalid").is_none());
    }
}

