use crate::cli::PsArgs;
use crate::container::runtime::{ContainerRuntime, RuntimeConfig};
use crate::error::Result;
use chrono_humanize::{Accuracy, HumanTime, Tense};

pub async fn execute(args: PsArgs) -> Result<()> {
    // Instanciate runtime
    let config = RuntimeConfig::from_env();
    let runtime = ContainerRuntime::new(config)?;

    // List containers
    let containers = runtime.list_containers(args.all).await?;

    if containers.is_empty() {
        if args.all {
            println!("No containers found.");
        } else {
            println!("No running containers found. use --all to see all of the containers.");
        }

        return Ok(());
    }

        // Print header
    println!("{:<12} {:<20} {:<15} {:<10} {:<20} {:<15}", 
             "CONTAINER ID", "IMAGE", "COMMAND", "STATUS", "CREATED", "NAMES");

    // print each container
    for container in containers {
        let command_str = if container.command.is_empty() {
            "".to_string()
        } else if container.command.len() == 1 {
            container.command[0].clone()
        } else {
            format!("{} {}", container.command[0], container.command[1..].join(" "))
        };

        // Truncate command if too long
        let command_display = if command_str.len() > 15 {
            format!("{}...", &command_str[..12])
        } else {
            command_str
        };

        let created_str = format_duration_since(container.created_at);
        let name = container.name.as_deref().unwrap_or("");

        println!("{:<12} {:<20} {:<15} {:<10} {:<20} {:<15}", 
                 &container.id[..12], 
                 container.blueprint, 
                 command_display, 
                 container.status, 
                 created_str, 
                 name);
    }

    Ok(())
}

fn format_duration_since(time: chrono::DateTime<chrono::Utc>) -> String {
    HumanTime::from(chrono::Utc::now() - time)
        .to_text_en(Accuracy::Rough, Tense::Past)
}

pub fn format_command_display(command: &[String], max_len: usize) -> String {
    let command_str = if command.is_empty() {
        "".to_string()
    } else if command.len() == 1 {
        command[0].clone()
    } else {
        format!("{} {}", command[0], command[1..].join(" "))
    };

    if command_str.len() > max_len {
        format!("{}...", &command_str[..max_len.saturating_sub(3)])
    } else {
        command_str
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::container::Container;

    #[test]
    fn test_format_command_display_empty() {
        let cmd: Vec<String> = vec![];
        assert_eq!(format_command_display(&cmd, 15), "");
    }

    #[test]
    fn test_format_command_display_single() {
        let cmd = vec!["bash".to_string()];
        assert_eq!(format_command_display(&cmd, 15), "bash");
    }

    #[test]
    fn test_format_command_display_truncated() {
        let cmd = vec!["echo".to_string(), "this is a very long command".to_string()];
        let result = format_command_display(&cmd, 15);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 15);
    }

    #[test]
    fn test_format_duration_since() {
        let now = chrono::Utc::now();
        let result = format_duration_since(now);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_format_duration_since_old_time() {
        let old_time = chrono::Utc::now() - chrono::Duration::hours(24);
        let result = format_duration_since(old_time);
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_execute_no_containers() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());

        let args = crate::cli::PsArgs {all: false};
        let result = execute(args).await;
        assert!(result.is_ok());

        std::env::remove_var("CUBO_ROOT");
    }

    #[tokio::test]
    async fn test_execute_with_all_flag() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());
        let args = crate::cli::PsArgs {all: true};
        let result = execute(args).await;
        assert!(result.is_ok());
        std::env::remove_var("CUBO_ROOT");
    }

    #[tokio::test]
    async fn test_execute_lists_created_container() {
        let temp_dir = TempDir::new().unwrap();
        let config = RuntimeConfig {
            root_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "test:latest".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
        ).with_name("test-ps-container".to_string());
        runtime.create_container(container).await.unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());
        let args = crate::cli::PsArgs {all:true};
        let result = execute(args).await;
        assert!(result.is_ok());
        std::env::remove_var("CUBO_ROOT");
    }

}
