use std::path::PathBuf;

use crate::cli::BuildArgs;
use crate::container::cubofile::Cubofile;
use crate::container::cubofile_toml::CubofileToml;
use crate::container::builder::ImageBuilder;
use crate::container::image_store::ImageStore;
use crate::error::{CuboError, Result};
use tracing::{info, error};

pub fn  detect_build_file(build_context: &PathBuf, specified_file: Option<&String>) -> Result<(PathBuf, bool)> {
    if let Some(file) = specified_file {
        let path = build_context.join(file);
        let is_toml = file.ends_with(".toml");
        Ok((path, is_toml))
    } else {
        let toml_path = build_context.join("Cubofile.toml");
        let text_path = build_context.join("Cubofile");

        if toml_path.exists() {
            Ok((toml_path, true))
        } else if text_path.exists() {
            Ok((text_path, false))
        } else {
            Err(CuboError::SystemError(
                "No Cubofile or Cubofile.toml found in the vuild context".to_string()
            ))
        }
    }
}

pub fn resolve_image_tag(path: &str, tag: Option<&String>) -> String {
    if let Some(t) = tag {
        t.clone()
    } else {
        let dir_name = PathBuf::from(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();
        format!("{}:latest", dir_name)
    }
}

pub async fn execute(args: BuildArgs) -> Result<()> {
    let build_context = PathBuf::from(&args.path);
    let (build_file_path, is_toml) = detect_build_file(&build_context, args.file.as_ref())?;

    info!("Building image from: {}", build_file_path.display());
    if !build_file_path.exists() {
        return Err(CuboError::SystemError(format!(
            "Build file not found: {}",
            build_file_path.display()
        )));
    }

    let image_tag = resolve_image_tag(&args.path, args.tag.as_ref());

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
                println!("Successfully built: {}", image_tag);
                println!();
                println!("Run with: cubo run {}", image_tag);
                Ok(())
            }
            Err(e) => {
                error!("Build failed: {}", e);
                println!("Build failed: {}", e);
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
                println!("Successfully built: {}", image_tag);
                println!();
                println!("Run with: cubo run {}", image_tag);
                Ok(())
            }
            Err(e) => {
                error!("Build failed: {}", e);
                println!("Build failed: {}", e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_resolve_tag_with_explicit_tag() {
        let tag = String::from("theimage:v1.0");
        let result = resolve_image_tag("/some/path/myproject", Some(&tag));
        assert_eq!(result, "theimage:v1.0");
    }

    #[test]
    fn test_resolve_image_tag_from_directory() {
        let result = resolve_image_tag("/some/path/myproject", None);
        assert_eq!(result, "myproject:latest");
    }

    #[test]
    fn test_resolve_image_tag_with_nested_path() {
        let result = resolve_image_tag("/home/user/projects/webapp", None);
        assert_eq!(result, "webapp:latest");
    }

    #[test]
    fn test_resolve_image_tag_with_current_dir() {
        let result = resolve_image_tag(".", None);
        assert_eq!(result, "unnamed:latest");
    }

    #[test]
    fn text_detect_build_file_with_specified_toml() {
        let temp = TempDir::new().unwrap();
        let build_context = temp.path().to_path_buf();
        let specified = String::from("custom.toml");

        let (path, is_toml) = detect_build_file(&build_context, Some(&specified)).unwrap();
        assert!(is_toml);
        assert_eq!(path, build_context.join("custom.toml"));
    }

    #[test]
    fn test_detect_build_file_with_specified_text() {
        let temp = TempDir::new().unwrap();
        let build_context = temp.path().to_path_buf();
        let specified = String::from("myCubofile");

        let (path, is_toml) = detect_build_file(&build_context, Some(&specified)).unwrap();
        assert!(!is_toml);
        assert_eq!(path, build_context.join("myCubofile"));
    }

    #[test]
    fn test_detect_build_file_prefers_toml() {
        let temp = TempDir::new().unwrap();
        let build_context = temp.path().to_path_buf();

        fs::write(build_context.join("Cubofile.toml"), "[image]\nbase = \"alpine\"").unwrap();
        fs::write(build_context.join("Cubofile"), "BASE alpine").unwrap();

        let (path, is_toml) = detect_build_file(&build_context, None).unwrap();
        assert!(is_toml);
        assert_eq!(path, build_context.join("Cubofile.toml"));
    }

    #[test]
    fn test_detect_build_file_falls_back_to_text() {
        let temp = TempDir::new().unwrap();
        let build_context = temp.path().to_path_buf();

        fs::write(build_context.join("Cubofile"), "BASE alpine").unwrap();

        let (path, is_toml) = detect_build_file(&build_context, None).unwrap();
        assert!(!is_toml);
        assert_eq!(path, build_context.join("Cubofile"));
    }

    #[test]
    fn test_detect_build_file_error_when_none_exists() {
        let temp = TempDir::new().unwrap();
        let build_context = temp.path().to_path_buf();

        let result = detect_build_file(&build_context, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No Cubofile"));
    }

    #[tokio::test]
    async fn test_execute_missing_build_context() {
        let args = BuildArgs {
            path: "/nonexistent/path/to/project".to_string(),
            tag: None,
            file: None,
            no_cache: false,
        };

        let result = execute(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_no_cubofile_in_context() {
        let temp = TempDir::new().unwrap();
        let args = BuildArgs {
            path: temp.path().to_string_lossy().to_string(),
            tag: None,
            file: None,
            no_cache: false,
        };

        let result = execute(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No Cubofile"));
    }

    #[tokio::test]
    async fn test_execute_specified_file_not_found() {
        let temp = TempDir::new().unwrap();
        let args = BuildArgs {
            path: temp.path().to_string_lossy().to_string(),
            tag: None,
            file: Some("nonexistent.toml".to_string()),
            no_cache: false,
        };

        let result = execute(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

}