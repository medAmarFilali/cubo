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
