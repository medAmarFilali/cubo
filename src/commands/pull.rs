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

pub fn parse_image_reference(image: &str) -> (Option<&str>, &str, &str) {
    let (image_part, tag) = if let Some(idx) = image.rfind(':') {
        let after_colon = &image[idx + 1..];
        if after_colon.contains('/') {
            (image, "latest")
        } else {
            (&image[..idx], after_colon)
        }
    } else {
        (image, "latest")
    };

    if let Some(idx) = image_part.find('/') {
        let potential_registry = &image_part[..idx];

        if potential_registry.contains('.') || potential_registry.contains(':') {
            return (Some(potential_registry), &image_part[idx + 1..], tag);
        }
    }
    (None, image_part, tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_image_reference_simple() {
        let (registry, repo, tag) = parse_image_reference("alpine");
        assert_eq!(registry, None);
        assert_eq!(repo, "alpine");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_reference_with_tag() {
        let (registry, repo, tag) = parse_image_reference("ubuntu:22.04");
        assert_eq!(registry, None);
        assert_eq!(repo, "ubuntu");
        assert_eq!(tag, "22.04");
    }

    #[test]
    fn test_parse_image_reference_with_namespace() {
        let (registry, repo, tag) = parse_image_reference("library/nginx:latest");
        assert_eq!(registry, None);
        assert_eq!(repo, "library/nginx");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_reference_with_registry() {
        let (registry, repo, tag) = parse_image_reference("ghcr.io/owner/repo:v1.0");
        assert_eq!(registry, Some("ghcr.io"));
        assert_eq!(repo, "owner/repo");
        assert_eq!(tag, "v1.0");
    }

    #[test]
    fn test_parse_image_reference_with_registry_port() {
        let (registry, repo, tag) = parse_image_reference("localhost:5000/theimage:test");
        assert_eq!(registry, Some("localhost:5000"));
        assert_eq!(repo, "theimage");
        assert_eq!(tag, "test");
    }

    #[test]
    fn test_parse_image_reference_docker_hub() {
        let (registry, repo, tag) = parse_image_reference("docker.io/library/alpine:3.18");
        assert_eq!(registry, Some("docker.io"));
        assert_eq!(repo, "library/alpine");
        assert_eq!(tag, "3.18");
    }

    #[tokio::test]
    async fn test_execute_creates_image_store_dir() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", temp_dir.path().to_string_lossy().to_string());

        let args = PullArgs {
            image: "nonexistent-registry.invalid/test:latest".to_string(),
        };
        let result = execute(args).await;
        assert!(result.is_err());
        assert!(temp_dir.path().join("images").exists());
        std::env::remove_var("CUBO_ROOT");
    }
}