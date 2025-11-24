use crate::cli::BlueprintArgs;
use crate::error::Result;
use tracing::{info, warn};

pub async fn execute(args: BlueprintArgs) -> Result<()> {
    info!("Listing blueprints (all: {})", args.all);

    warn!("Blueprint command is not yet implemented");
    println!("Blueprint management functionality is planned for a future release.");
    println!("Currently, Cubo creates basic rootfs environments on-the-fly when running containers.");
    println!("Future versions will support proper blueprint layers and management.");

    // Just a placeholder for now
    println!("\nREPOSITORY          TAG       IMAGE ID       CREATED       SIZE");
    println!("<none>              <none>    <none>         <none>        <none>");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::BlueprintArgs;

    #[tokio::test]
    async fn test_execute_placeholder() {
        let args = BlueprintArgs {
            all: "true".to_string(),
        };

        let result = execute(args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_with_false() {
        let args = BlueprintArgs {
            all: "false".to_string(),
        };

        let result = execute(args).await;
        assert!(result.is_ok());
    }
}