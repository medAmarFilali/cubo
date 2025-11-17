use std::path::PathBuf;

use crate::cli::BuildArgs;
use crate::container::cubofile::Cubofile;
use crate::container::cubofile_toml::CubofileToml;
use crate::container::builder::ImageBuilder;
use crate::container::image_store::ImageStore;
use crate::error::{CuboError, Result};
use tracing::{info, error};

pub async fn execute(args: BuildArgs) -> Result<()> {
    let build_context = PathBuf::from(&args.path);

    let (build_file_path, is_toml) = if let Some(ref file) = args.file {
        let path = build_context.join(file);
        let is_toml = file.ends_with(".toml");
        (path, is_toml)
    } else {
        let toml_path = build_context.join("Cubofile.toml");
        let text_path = build_context.join("Cubofile");

        if toml_path.exists() {
            (toml_path, true)
        } else if text_path.exists() {
            (text_path, false)
        } else {
            return Err(CuboError::SystemError(
                "No Cubofile or Cubofile.toml found in build context".to_string()
            ));
        }
    };

    info!("Building image from: {}", build_file_path.display());
    if !build_file_path.exists() {
        return Err(CuboError::SystemError(format!(
            "Build file not found: {}",
            build_file_path.display()
        )));
    }

    let image_tag = if let Some(ref tag) = args.tag {
        tag.clone()
    } else {
        let dir_name = PathBuf::from(&args.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();
        format!("{}:latest", dir_name)
    };

    let root_dir = std::env::var("CUBO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/cubo"));

    let image_store = ImageStore::new(root_dir.join("images"))?;

    let builder = ImageBuilder::new(&image_store, build_context.clone());

    if is_toml {
        info!("Parsing Cubofile.toml...");
        let cubofile = CubofileToml::from_file(&build_file_path)?;

        println!("Building image: {}", image_tag);
        println!("Base image: {}", cubofile.base_image());
        println!("Build context: {}", args.path);
        println!("Format: TOML");
        println!();

        match builder.build_from_toml(&cubofile, &image_tag).await {
            Ok(_) => {
                println!("✓ Successfully built: {}", image_tag);
                println!();
                println!("Run with: cubo run {}", image_tag);
                Ok(())
            }
            Err(e) => {
                error!("Build failed: {}", e);
                println!("✗ Build failed: {}", e);
                println!();
                println!("Make sure:");
                println!("  1. Base image is imported: cubo image import <ref> <tar>");
                println!("  2. You have root privileges (needed for chroot)");
                println!("  3. All COPY source files exist in build context");
                Err(e)
            }
        }
    } else {
        info!("Parsing Cubofile...");
        let cubofile = Cubofile::from_file(&build_file_path)?;

        if cubofile.base_image().is_none() {
            return Err(CuboError::InvalidConfiguration(
                "Cubofile must contain a BASE instruction".to_string()
            ));
        }

        println!("Building image: {}", image_tag);
        println!("Base image: {}", cubofile.base_image().unwrap());
        println!("Build context: {}", args.path);
        println!("Format: Text");
        println!();

        match builder.build(&cubofile, &image_tag).await {
            Ok(_) => {
                println!("✓ Successfully built: {}", image_tag);
                println!();
                println!("Run with: cubo run {}", image_tag);
                Ok(())
            }
            Err(e) => {
                error!("Build failed: {}", e);
                println!("✗ Build failed: {}", e);
                println!();
                println!("Make sure:");
                println!("  1. Base image is imported: cubo image import <ref> <tar>");
                println!("  2. You have root privileges (needed for chroot)");
                println!("  3. All COPY source files exist in build context");
                Err(e)
            }
        }
    }
}