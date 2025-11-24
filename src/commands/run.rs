use crate::cli::RunArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::container::{Container, VolumeMount, PortMapping, Protocol};
use crate::container::image_store::ImageStore;
use crate::error::Result;
use tracing::{info, warn, error};

pub async fn execute(args: RunArgs) -> Result<()> {
    info!("Running container with blueprint: {}", args.blueprint);

    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config.clone())?;

    let image_store_path = config.root_dir.join("images");
    let image_store = ImageStore::new(image_store_path)?;

    let command = if let Some(cmd) = args.command {
        cmd
    } else {
        match image_store.get_config(&args.blueprint) {
            Ok(img_config) => {
                if let Some(cmd) = img_config.cmd {
                    info!("Using default CMD from image: {:?}", cmd);
                    cmd
                } else {
                    warn!("No CMD in image config, defaulting to /bin/sh");
                    vec!["/bin/sh".to_string()]
                }
            }
            Err(e) => {
                warn!("Failed to load image config: {}, defaulting to /bin/sh", e);
                vec!["/bin/sh".to_string()]
            }
        }
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

    let detached = !args.interactive;

    match runtime.start_container(&container_id, detached).await {
        Ok(_) => {
            if detached {
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
    fn test_parse_env_var_empty_value() {
        let result = parse_env_var("EMPTY=");
        assert_eq!(result, Some(("EMPTY".to_string(), "".to_string())));
    }

    #[test]
    fn test_parse_env_var_value_with_equals() {
        let result = parse_env_var("DATABASE_URL=postgres://user=admin");
        assert_eq!(result, Some(("DATABASE_URL".to_string(), "postgres://user=admin".to_string())));
    }

    #[test]
    fn test_parse_env_var_empty_string() {
        let result = parse_env_var("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_env_var_only_equals() {
        let result = parse_env_var("=");
        assert_eq!(result, Some(("".to_string(), "".to_string())));
    }

    #[test]
    fn test_parse_env_var_complex_value() {
        let result = parse_env_var("JSON={\"key\":\"value\"}");
        assert_eq!(result, Some(("JSON".to_string(), "{\"key\":\"value\"}".to_string())));
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
    fn test_parse_volume_read_write_explicit() {
        let volume = parse_volume("/data:/app/data:rw").unwrap();
        assert_eq!(volume.host_path, "/data");
        assert_eq!(volume.container_path, "/app/data");
        assert!(!volume.read_only); // "rw" != "ro", so read_only is false
    }

    #[test]
    fn test_parse_volume_single_path() {
        let result = parse_volume("/single/path");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_volume_too_many_parts() {
        let result = parse_volume("/a:/b:ro:extra");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_volume_empty_string() {
        let result = parse_volume("");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_volume_with_spaces_in_path() {
        let volume = parse_volume("/path with spaces:/container/path").unwrap();
        assert_eq!(volume.host_path, "/path with spaces");
        assert_eq!(volume.container_path, "/container/path");
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

    #[test]
    fn test_parse_port_tcp_explicit() {
        let port = parse_port("3000:3000/tcp").unwrap();
        assert_eq!(port.host_port, 3000);
        assert_eq!(port.container_port, 3000);
        assert!(matches!(port.protocol, Protocol::Tcp));
    }

    #[test]
    fn test_parse_port_invalid_protocol_defaults_tcp() {
        let port = parse_port("8080:80/xyz").unwrap();
        assert_eq!(port.host_port, 8080);
        assert_eq!(port.container_port, 80);
        assert!(matches!(port.protocol, Protocol::Tcp));
    }

    #[test]
    fn test_parse_port_invalid_host_number() {
        let result = parse_port("abc:80");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_port_invalid_container_number() {
        let result = parse_port("8080:xyz");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_port_empty_string() {
        let result = parse_port("");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_port_only_colon() {
        let result = parse_port(":");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_port_single_number() {
        let result = parse_port("8080");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_port_uppercase_protocol() {
        let udp_port = parse_port("53:53/UDP").unwrap();
        assert!(matches!(udp_port.protocol, Protocol::Udp));

        let tcp_port = parse_port("80:80/TCP").unwrap();
        assert!(matches!(tcp_port.protocol, Protocol::Tcp));
    }

    #[test]
    fn test_parse_port_high_port_numbers() {
        let port = parse_port("65535:65535").unwrap();
        assert_eq!(port.host_port, 65535);
        assert_eq!(port.container_port, 65535);
    }

    #[test]
    fn test_parse_port_low_port_numbers() {
        let port = parse_port("1:1").unwrap();
        assert_eq!(port.host_port, 1);
        assert_eq!(port.container_port, 1);
    }
}

