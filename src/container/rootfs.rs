use std::fs;
use std::path::Path;
use std::process::Command;

use tracing::{debug, error, info, warn};

use crate::error::{CuboError, Result};
use super::image_store::ImageStore;

pub struct RootfsBuilder<'a> {
    image_store: &'a ImageStore,
}

impl<'a> RootfsBuilder<'a> {
    pub fn new(image_store: &'a ImageStore) -> Self {
        Self { image_store }
    }

    pub fn build_from_image(&self, image_ref: &str, target: &Path) -> Result<()> {
        info!("Building rootfs for {} at {}", image_ref, target.display());

        fs::create_dir_all(target)
            .map_err(|e| CuboError::SystemError(format!("Failed to create rootfs directory: {}", e)))?;

        let layers  = self.image_store.get_layers(image_ref)?;

        if layers.is_empty() {
            return Err(CuboError::SystemError(format!("Image {} has no layers", image_ref)));
        }

        debug!("Extrac ting {} layers for {}", layers.len(), image_ref);

        for (idx, layer_path ) in layers .iter().enumerate() {
            debug!("Extracting layer {}/{}: {}", idx + 1, layers.len(), layer_path.display());
            self.extract_layer(layer_path, target)?;
        }

        self.ensure_essential_dirs(target)?;

        info!("Successfully built rootfs for {}", image_ref);
        Ok(())
    }

    fn extract_layer(&self, layer_path: &Path, target: &Path) -> Result<()> {
        if !layer_path.exists() {
            return Err(CuboError::SystemError(format!("Layer file does not exist: {}", layer_path.display())));
        }

        let is_gzip = layer_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s == "gz" || s == "tgz")
            .unwrap_or(false);

        let mut cmd = Command::new("tar");

        if is_gzip {
            cmd.arg("-xzf");
        } else {
            cmd.arg("-xf");
        }

        cmd.arg(layer_path)
            .arg("-C")
            .arg(target)
            .arg("--no-same-owner")
            .arg("--no-same-permissions");

        debug!("Running: {:?}", cmd);

        let output = cmd.output()
            .map_err(|e| CuboError::SystemError(format!("Failed to execute tar command: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CuboError::SystemError(format!(
                "Failed to extract layer {}: {}",
                layer_path.display(),
                stderr
            )));
        }

        Ok(())
    }

    fn ensure_essential_dirs(&self, rootfs: &Path) -> Result<()> {
        let dirs = [
            "dev", "proc", "sys", "tmp",
            "etc", "var", "var/log", "var/tmp",
        ];

        for dir in &dirs {
            let dir_path = rootfs.join(dir);
            if !dir_path.exists() {
                debug!("Creating missing directory: {}", dir_path.display());
                fs::create_dir_all(&dir_path)
                    .map_err(|e| CuboError::SystemError(format!(
                        "Failed to create directory {}: {}", dir, e
                    )))?;
            }
        }

        Ok(())
    }

    pub fn create_minimal_rootfs(&self, target: &Path) -> Result<()> {
        warn!("Creating minimal rootfs at {} (no image)", target.display());

        fs::create_dir_all(target)
            .map_err(|e| CuboError::SystemError(format!("Failed to create rootfs directory: {}", e)))?;

        let dirs = [
            "bin", "etc", "lib", "lib64", "usr", "var", "tmp",
            "dev", "proc", "sys", "mnt", "opt", "root", "home",
            "usr/bin", "usr/lib", "usr/local", "usr/share",
            "var/log", "var/tmp", "var/run",
        ];

        for dir in &dirs {
            let dir_path = target.join(dir);
            fs::create_dir_all(&dir_path)
                .map_err(|e| CuboError::SystemError(format!(
                    "Failed to create directory {}: {}", dir, e
                )))?;
        }

        self.copy_essential_binaries(target)?;

        Ok(())
    }

    fn copy_essential_binaries(&self, rootfs: &Path) -> Result<()> {
        let essential_binaries = [
            "/bin/sh",
            "/bin/bash",
            "/bin/ls",
            "/bin/cat",
            "/bin/echo",
            "/bin/mkdir",
            "/bin/rm",
        ];

        for binary in &essential_binaries {
            let binary_path = Path::new(binary);
            if binary_path.exists() {
                let dest_path = rootfs.join(binary.trim_start_matches('/'));
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| CuboError::SystemError(format!(
                            "Failed to create directory: {}", e
                        )))?;
                }

                if let Err(e) = fs::copy(binary_path, &dest_path) {
                    debug!("Failed to copy {}: {}", binary, e);
                } else {
                    debug!("Copied {} to rootfs", binary);
                }
            }
        }

        Ok(())
    }

}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs::File;
    use std::io::Write;

    fn create_test_tar(path: &Path, content: &str) -> Result<()> {
        // Create a simple tar file for testing
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let mut f = File::create(&test_file).unwrap();
        f.write_all(content.as_bytes()).unwrap();

        let output = Command::new("tar")
            .arg("-cf")
            .arg(path)
            .arg("-C")
            .arg(temp_dir.path())
            .arg("test.txt")
            .output()
            .map_err(|e| CuboError::SystemError(format!("Failed to create test tar: {}", e)))?;

        if !output.status.success() {
            return Err(CuboError::SystemError("Failed to create test tar".to_string()));
        }

        Ok(())
    }

    #[test]
    fn test_ensure_essential_dirs() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        builder.ensure_essential_dirs(&rootfs).unwrap();

        assert!(rootfs.join("dev").exists());
        assert!(rootfs.join("proc").exists());
        assert!(rootfs.join("sys").exists());
        assert!(rootfs.join("tmp").exists());
    }

    #[test]
    fn test_create_minimal_rootfs() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        builder.create_minimal_rootfs(&rootfs).unwrap();

        assert!(rootfs.exists());
        assert!(rootfs.join("bin").exists());
        assert!(rootfs.join("etc").exists());
        assert!(rootfs.join("usr/bin").exists());
    }

    #[test]
    fn test_extract_layer() {
        let tmp = TempDir::new().unwrap();
        let tar_path = tmp.path().join("layer.tar");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&rootfs).unwrap();

        // Create a test tar file
        create_test_tar(&tar_path, "hello from layer").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        builder.extract_layer(&tar_path, &rootfs).unwrap();

        let extracted_file = rootfs.join("test.txt");
        assert!(extracted_file.exists());

        let content = fs::read_to_string(extracted_file).unwrap();
        assert_eq!(content, "hello from layer");
    }

    #[test]
    fn test_extract_layer_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);
        let result = builder.extract_layer(Path::new("/nonexistent/layer.tar"), &rootfs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_build_from_image_not_found() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);
        let result = builder.build_from_image("nonexistent:image", &rootfs);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_essential_binaries() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);
        let result = builder.copy_essential_binaries(&rootfs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_minimal_rootfs_directory_structure() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        let image_Store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_Store);
        builder.create_minimal_rootfs(&rootfs).unwrap();
        let expected_dirs = ["bin", "etc", "lib", "usr", "var", "tmp", "dev", "proc", "sys"];
        for dir in &expected_dirs {
            assert!(rootfs.join(dir).exists(), "Directory {} should exist", dir);
        }
    }

    #[test]
    fn test_ensure_essential_dirs_creates_nested() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);
        builder.ensure_essential_dirs(&rootfs).unwrap();
        assert!(rootfs.join("var/log").exists());
        assert!(rootfs.join("var/tmp").exists());
    }

    #[test]
    fn test_build_from_image_with_layers() {
        use crate::container::image_store::{ImageManifest, ImageConfig};

        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();

        let layer_path = tmp.path().join("images/blobs/layer.tar");
        fs::create_dir_all(layer_path.parent().unwrap()).unwrap();
        create_test_tar(&layer_path, "test content").unwrap();

        let manifest = ImageManifest {
            reference: "test:latest".to_string(),
            layers: vec![layer_path.to_string_lossy().to_string()],
            config: ImageConfig {
                cmd: Some(vec!["/bin/sh".to_string()]),
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };
        image_store.save_manifest(&manifest).unwrap();

        let builder = RootfsBuilder::new(&image_store);
        let result = builder.build_from_image("test:latest", &rootfs);
        assert!(result.is_ok());

        assert!(rootfs.join("test.txt").exists());
        assert!(rootfs.join("dev").exists());
        assert!(rootfs.join("proc").exists());
    }

    #[test]
    fn test_build_from_image_with_empty_layers() {
        use crate::container::image_store::{ImageManifest, ImageConfig};

        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();

        let manifest = ImageManifest {
            reference: "empty:latest".to_string(),
            layers: vec![],
            config: ImageConfig {
                cmd: None,
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };
        image_store.save_manifest(&manifest).unwrap();

        let builder = RootfsBuilder::new(&image_store);
        let result = builder.build_from_image("empty:latest", &rootfs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no layers"));
    }

    #[test]
    fn test_extract_layer_gzip() {
        let tmp = TempDir::new().unwrap();
        let tar_path = tmp.path().join("layer.tar");
        let gz_path = tmp.path().join("layer.tar.gz");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&rootfs).unwrap();

        // Create a test tar file
        create_test_tar(&tar_path, "gzip test").unwrap();

        // Gzip it
        let output = Command::new("gzip")
            .arg("-c")
            .arg(&tar_path)
            .output()
            .unwrap();
        fs::write(&gz_path, &output.stdout).unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        let result = builder.extract_layer(&gz_path, &rootfs);
        assert!(result.is_ok());

        let extracted_file = rootfs.join("test.txt");
        assert!(extracted_file.exists());
    }

    #[test]
    fn test_extract_layer_tgz_extension() {
        let tmp = TempDir::new().unwrap();
        let tar_path = tmp.path().join("layer.tar");
        let tgz_path = tmp.path().join("layer.tgz");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&rootfs).unwrap();

        create_test_tar(&tar_path, "tgz test").unwrap();

        let output = Command::new("gzip")
            .arg("-c")
            .arg(&tar_path)
            .output()
            .unwrap();
        fs::write(&tgz_path, &output.stdout).unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        let result = builder.extract_layer(&tgz_path, &rootfs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_from_image_multiple_layers() {
        use crate::container::image_store::{ImageManifest, ImageConfig};

        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();

        let layer1_path = tmp.path().join("images/blobs/layer1.tar");
        let layer2_path = tmp.path().join("images/blobs/layer2.tar");
        fs::create_dir_all(layer1_path.parent().unwrap()).unwrap();

        create_test_tar(&layer1_path, "layer 1 content").unwrap();

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("layer2.txt");
        fs::write(&test_file, "layer 2 content").unwrap();
        let _ = Command::new("tar")
            .arg("-cf")
            .arg(&layer2_path)
            .arg("-C")
            .arg(temp_dir.path())
            .arg("layer2.txt")
            .output();

        let manifest = ImageManifest {
            reference: "multi:latest".to_string(),
            layers: vec![
                layer1_path.to_string_lossy().to_string(),
                layer2_path.to_string_lossy().to_string(),
            ],
            config: ImageConfig {
                cmd: Some(vec!["/bin/sh".to_string()]),
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };
        image_store.save_manifest(&manifest).unwrap();

        let builder = RootfsBuilder::new(&image_store);
        let result = builder.build_from_image("multi:latest", &rootfs);
        assert!(result.is_ok());

        assert!(rootfs.join("test.txt").exists());
        assert!(rootfs.join("layer2.txt").exists());
    }

    #[test]
    fn test_rootfs_builder_new() {
        let tmp = TempDir::new().unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let _builder = RootfsBuilder::new(&image_store);
    }

    #[test]
    fn test_ensure_essential_dirs_already_exist() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(rootfs.join("dev")).unwrap();
        fs::create_dir_all(rootfs.join("proc")).unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = RootfsBuilder::new(&image_store);

        let result = builder.ensure_essential_dirs(&rootfs);
        assert!(result.is_ok());
    }

}
