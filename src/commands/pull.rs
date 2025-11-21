use std::path::PathBuf;

use crate::cli::PullArgs;
use crate::container::image_store::ImageStore;
use crate::container::registry::RegistryClient;
use crate::error::Result;
use tracing::info;

pub async fn execute(args: PullArgs) -> Result<()> {
    info!("Pulling image: {}", args.image);

    // Get root directory from environment
    let root_dir = std::env::var("CUBO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/cubo"));

    let image_store = ImageStore::new(root_dir.join("images"))?;

    let registry_client = RegistryClient::new(image_store);

    println!("Pulling image: {}", args.image);
    println!();

    match registry_client.pull(&args.image).await {
        Ok(_) => {
            println!("Successfully pulled: {}", args.image);
            println!();
            println!("Use with: ");
            println!("  cubo run {}", args.image);
            println!("  cubo build (with BASE {})", args.image);
            Ok(())
        }
        Err(e) => {
            eprintln!("Pull failed: {}", e);
            eprintln!();
            eprintln!("Common issues: ");
            eprintln!("  - Check you internet connection");
            eprintln!("  - Verify the image name is correct");
            eprintln!("  - For private images, authentication is not yet supported");
            Err(e)
        }
    }
}