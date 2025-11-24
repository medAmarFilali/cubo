use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

use crate::error::{CuboError, Result};
use super::cubofile::{Cubofile, Instruction};
use super::cubofile_toml::CubofileToml;
use super::image_store::{ImageStore, ImageManifest, ImageConfig};
use super::rootfs::RootfsBuilder;

pub struct ImageBuilder<'a> {
    image_store: &'a ImageStore,
    build_context: PathBuf,
}

impl<'a> ImageBuilder<'a> {
    pub fn new(image_store: &'a ImageStore, build_context: PathBuf) -> Self {
        Self {
            image_store,
            build_context,
        }
    }

    pub async fn build(&self, cubofile: &Cubofile, image_ref: &str) -> Result<()> {
        info!("Building image: {}", image_ref);

        let base_image = cubofile.base_image().ok_or_else(|| {
            CuboError::InvalidConfiguration("Cubofile must start with BASE instruction".to_string())
        })?;

        info!("Base image: {}", base_image);

        self.ensure_image_available(&base_image).await?;

        let temp_dir = tempfile::tempdir()
            .map_err(|e| CuboError::SystemError(format!("Failed to create temp dir: {}", e)))?;
        let work_rootfs = temp_dir.path().join("rootfs");

        info!("Extracting base image into working directory");
        let rootfs_builder = RootfsBuilder::new(self.image_store);
        rootfs_builder.build_from_image(&base_image, &work_rootfs)?;

        let base_config = self.image_store.get_config(&base_image)?;
        let mut image_config = base_config;

        for (idx, instruction) in cubofile.instructions.iter().enumerate() {
            match instruction {
                Instruction::Base { .. } => {
                    debug!("Step {}: BASE (already applied)", idx + 1);
                }

                Instruction::Run { command } => {
                    info!("Step {}: RUN {}", idx + 1, command);
                    self.execute_run(&work_rootfs, command)?;
                }

                Instruction::Copy { src, dest } => {
                    info!("Step {}: COPY {} {}", idx + 1, src, dest);
                    self.execute_copy(&work_rootfs, src, dest)?;
                }

                Instruction::Env { key, value } => {
                    info!("Step {}: ENV {}={}", idx + 1, key, value);
                    let mut env_vars = image_config.env.unwrap_or_default();
                    env_vars.push(format!("{}={}", key, value));
                    image_config.env = Some(env_vars);
                }

                Instruction::Workdir { path } => {
                    info!("Step {}: WORKDIR {}", idx + 1, path);
                    image_config.working_dir = Some(path.clone());
                }

                Instruction::Cmd { command } => {
                    info!("Step {}: CMD {:?}", idx + 1, command);
                    image_config.cmd = Some(command.clone());
                }

                Instruction::Comment => {
                    // Ignore comments
                }
            }
        }

        info!("Creating image layer from built rootfs");
        let layer_tar = temp_dir.path().join("layer.tar");
        self.create_layer_tar(&work_rootfs, &layer_tar)?;

        let safe_name = image_ref.replace(':', "_");
        let final_layer_path = self.image_store_root().join("blobs").join(format!("{}.tar", safe_name));

        fs::create_dir_all(final_layer_path.parent().unwrap())
            .map_err(|e| CuboError::SystemError(format!("Failed to create blobs dir: {}", e)))?;
        fs::copy(&layer_tar, &final_layer_path)
            .map_err(|e| CuboError::SystemError(format!("Failed to copy layer: {}", e)))?;

        let manifest = ImageManifest {
            reference: image_ref.to_string(),
            layers: vec![final_layer_path.to_string_lossy().to_string()],
            config: image_config,
        };

        self.save_manifest(&manifest)?;

        info!("Successfully built image: {}", image_ref);
        Ok(())
    }


    pub async fn build_from_toml(&self, cubofile: &CubofileToml, image_ref: &str) -> Result<()> {
        info!("BUilding image from TOML: {}", image_ref);

        let base_image = &cubofile.image.base;
        info!("Base image: {}", base_image);

        self.ensure_image_available(base_image).await?;

        let temp_dir = tempfile::tempdir()
            .map_err(|e| CuboError::SystemError(format!("Failed to create temp dir: {}", e)))?;
        let work_rootfs = temp_dir.path().join("rootfs");

        info!("extracting base image into working directory");
        let rootfs_builder = RootfsBuilder::new(self.image_store);
        rootfs_builder.build_from_image(base_image, &work_rootfs)?;

        let base_config = self.image_store.get_config(base_image)?;
        let mut image_config = base_config;

        for (idx, run_step) in cubofile.run.iter().enumerate() {
            info!("Step {}: Run {}", idx + 1, run_step.command);
            self.execute_run(&work_rootfs, &run_step.command)?;
        }

        for (idx, copy_step) in cubofile.copy.iter().enumerate() {
            info!("Step {}: Copy {} {}", idx + 1, copy_step.src, copy_step.dest);
            self.execute_copy(&work_rootfs, &copy_step.src, &copy_step.dest)?;
        }

        if let Some(ref workdir) = &cubofile.config.workdir {
            info!("Setting WORKDIR to {}", workdir);
            image_config.working_dir = Some(workdir.clone());
        }

        if let Some(ref cmd ) = &cubofile.config.cmd {
            info!("Setting CMD: {:?}", cmd);
            image_config.cmd = Some(cmd.clone());
        }

        if !cubofile.config.env.is_empty() {
            let mut env_vars = image_config.env.unwrap_or_default();
            for (key, value) in &cubofile.config.env {
                info!("Settings ENV {}={}", key, value);
                env_vars.push(format!("{}={}", key, value));
            }
            image_config.env = Some(env_vars);
        }

        if !cubofile.config.expose.is_empty() {
            info!("Settings EXPOSE: {:?}", cubofile.config.expose);
            image_config.exposed_ports = Some(cubofile.config.expose.clone());
        }

        info!("Creating image layer from built rootfs");
        let layer_tar = temp_dir.path().join("layer.tar");
        self.create_layer_tar(&work_rootfs, &layer_tar)?;

        let safe_name = image_ref.replace(":", "_");
        let final_layer_path = self.image_store_root().join("blobs").join(format!("{}.tar", safe_name));

        fs::create_dir_all(final_layer_path.parent().unwrap())
            .map_err(|e| CuboError::SystemError(format!("Failed to create blobs dir: {}", e)))?;

        fs::copy(&layer_tar, &final_layer_path)
            .map_err(|e| CuboError::SystemError(format!("Failed to copy layer: {}", e)))?;

        let manifest = ImageManifest {
            reference: image_ref.to_string(),
            layers: vec![final_layer_path.to_string_lossy().to_string()],
            config: image_config,
        };

        self.save_manifest(&manifest)?;

        info!("Successfully built image: {}", image_ref);
        Ok(())
    }

    async fn ensure_image_available(&self, image_ref: &str) -> Result<()> {
        if self.image_store.has_image(image_ref) {
            debug!("Image {} already available locally", image_ref);
            return Ok(());
        }

        info!("Base image {} not found locally, pulling from registry...", image_ref);
        println!("Pulling base image: {}", image_ref);

        use super::registry::RegistryClient;
        let registry_client = RegistryClient::new(ImageStore::new(self.image_store_root())?);

        registry_client.pull(image_ref).await?;

        println!("Base image ready: {}", image_ref);
        Ok(())
    }

    /// Execute a RUN instruction
    fn execute_run(&self, rootfs: &Path, command: &str) -> Result<()> {
        // Use chroot to run command in the rootfs
        // For simplicity, we'll use /bin/sh from the rootfs
        let sh_path = rootfs.join("bin/sh");

        if !sh_path.exists() {
            warn!("No /bin/sh in rootfs, trying /bin/bash");
            let bash_path = rootfs.join("bin/bash");
            if !bash_path.exists() {
                return Err(CuboError::SystemError(
                    "No shell found in rootfs (/bin/sh or /bin/bash)".to_string(),
                ));
            }
        }

        let resolv_conf_dest = rootfs.join("etc/resolv.conf");
        if let Err(e) = fs::copy("/etc/resolv.conf", &resolv_conf_dest) {
            warn!("Failed to copy /etc/resolv.conf: {} - network may not work", e);
        }

        let tmp_dir = rootfs.join("tmp");
        if let Err(e) = fs::create_dir_all(&tmp_dir) {
            warn!("Failed to create /tmp: {}", e);
        } else {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o1777));
        }

        let dev_dir = rootfs.join("dev");
        let _ = fs::create_dir_all(&dev_dir);

        let mount_result = Command::new("mount")
            .args(["--bind", "/dev", dev_dir.to_str().unwrap()])
            .output();

        let dev_mounted = mount_result.is_ok() && mount_result.as_ref().unwrap().status.success();
        if !dev_mounted {
            warn!("Failed to bind mount /dev - some commands may fail");
        }

        let proc_dir = rootfs.join("proc");
        let _ = fs::create_dir_all(&proc_dir);
        let proc_mount_result = Command::new("mount")
            .args(["-t", "proc", "proc", proc_dir.to_str().unwrap()])
            .output();
        let proc_mounted = proc_mount_result.is_ok() && proc_mount_result.as_ref().unwrap().status.success();

        let output = Command::new("chroot")
            .arg(rootfs)
            .arg("/bin/sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| CuboError::SystemError(format!("Failed to execute chroot: {}", e)));

        if proc_mounted {
            let _ = Command::new("umount").arg(&proc_dir).output();
        }
        if dev_mounted {
            let _ = Command::new("umount").arg(&dev_dir).output();
        }

        let output = output?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CuboError::SystemError(format!(
                "RUN command failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            debug!("RUN output: {}", stdout);
        }

        Ok(())
    }

    /// Execute a COPY instruction
    fn execute_copy(&self, rootfs: &Path, src: &str, dest: &str) -> Result<()> {
        let src_path = self.build_context.join(src);

        if !src_path.exists() {
            return Err(CuboError::SystemError(format!(
                "Source path does not exist: {}",
                src_path.display()
            )));
        }

        // Destination is relative to rootfs
        let dest_path = if dest.starts_with('/') {
            rootfs.join(dest.trim_start_matches('/'))
        } else {
            rootfs.join(dest)
        };

        // Create parent directory
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CuboError::SystemError(format!("Failed to create dest directory: {}", e)))?;
        }

        // Copy file or directory
        if src_path.is_file() {
            fs::copy(&src_path, &dest_path)
                .map_err(|e| CuboError::SystemError(format!("Failed to copy file: {}", e)))?;
        } else if src_path.is_dir() {
            self.copy_dir_recursive(&src_path, &dest_path)?;
        }

        debug!("Copied {} to {}", src_path.display(), dest_path.display());
        Ok(())
    }

    /// Recursively copy a directory
    fn copy_dir_recursive(&self, src: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)
            .map_err(|e| CuboError::SystemError(format!("Failed to create directory: {}", e)))?;

        for entry in fs::read_dir(src)
            .map_err(|e| CuboError::SystemError(format!("Failed to read directory: {}", e)))?
        {
            let entry = entry
                .map_err(|e| CuboError::SystemError(format!("Failed to read entry: {}", e)))?;
            let src_path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if src_path.is_file() {
                fs::copy(&src_path, &dest_path)
                    .map_err(|e| CuboError::SystemError(format!("Failed to copy file: {}", e)))?;
            } else if src_path.is_dir() {
                self.copy_dir_recursive(&src_path, &dest_path)?;
            }
        }

        Ok(())
    }

    /// Create a tar archive from a rootfs directory
    fn create_layer_tar(&self, rootfs: &Path, output: &Path) -> Result<()> {
        let output_cmd = Command::new("tar")
            .arg("-cf")
            .arg(output)
            .arg("-C")
            .arg(rootfs)
            .arg(".")
            .output()
            .map_err(|e| CuboError::SystemError(format!("Failed to create tar: {}", e)))?;

        if !output_cmd.status.success() {
            let stderr = String::from_utf8_lossy(&output_cmd.stderr);
            return Err(CuboError::SystemError(format!(
                "Failed to create layer tar: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Get the image store root directory
    fn image_store_root(&self) -> PathBuf {
        // This is a bit hacky - we need access to the image store's root
        // In a real implementation, we'd expose this via ImageStore
        // For now, we'll assume it's in the same structure
        std::env::var("CUBO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/cubo"))
            .join("images")
    }

    /// Save manifest (duplicated from ImageStore for now)
    fn save_manifest(&self, manifest: &ImageManifest) -> Result<()> {
        let safe_name = manifest.reference.replace(':', "_");
        let manifest_path = self.image_store_root()
            .join("manifests")
            .join(format!("{}.json", safe_name));

        fs::create_dir_all(manifest_path.parent().unwrap())
            .map_err(|e| CuboError::SystemError(format!("Failed to create manifests dir: {}", e)))?;

        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| CuboError::SystemError(format!("Failed to serialize manifest: {}", e)))?;

        fs::write(&manifest_path, json)
            .map_err(|e| CuboError::SystemError(format!("Failed to write manifest: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_builder_creation() {
        let tmp = TempDir::new().unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        // Just verify it compiles and creates
        assert_eq!(builder.build_context, tmp.path());
    }

    #[test]
    fn test_copy_dir_recursive() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file1.txt"), "content1").unwrap();
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("subdir/file2.txt"), "content2").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        builder.copy_dir_recursive(&src, &dest).unwrap();

        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("subdir/file2.txt").exists());
        assert_eq!(fs::read_to_string(dest.join("file1.txt")).unwrap(), "content1");
    }

    #[test]
    fn test_copy_dir_recursive_creates_dest() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("nested/deep/dest");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("test.txt"), "test content").unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());
        builder.copy_dir_recursive(&src, &dest).unwrap();
        assert!(dest.exists());
        assert!(dest.join("test.txt").exists());
    }

    #[test]
    fn test_copy_single_file() {
        let tmp = TempDir::new().unwrap();
        let src_file = tmp.path().join("source.txt");
        let dest_file = tmp.path().join("dest/copied.txt");
        fs::write(&src_file, "file content").unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());
        fs::create_dir_all(dest_file.parent().unwrap()).unwrap();
        fs::copy(&src_file, &dest_file).unwrap();
        assert!(dest_file.exists());
        assert_eq!(fs::read_to_string(&dest_file).unwrap(), "file content");
    }

    #[test]
    fn test_builder_build_context_path() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("my/build/context");
        fs::create_dir_all(&context).unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context.clone());
        assert_eq!(builder.build_context, context);
    }

    #[tokio::test]
    async fn test_build_missing_base_image() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        fs::create_dir_all(&context).unwrap();
        let cubofile_content = "BASE nonexistent:image\nRUN echo hello";
        fs::write(context.join("Cubofile"), cubofile_content).unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);
        let cubofile = crate::container::cubofile::Cubofile::from_string(cubofile_content).unwrap();
        let result = builder.build(&cubofile, "test:build").await;
        assert!(result.is_err());
    }

     #[test]
    fn test_execute_copy_file() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&context).unwrap();
        fs::create_dir_all(&rootfs).unwrap();
        fs::write(context.join("app.txt"), "application data").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let result = builder.execute_copy(&rootfs, "app.txt", "/app/app.txt");
        assert!(result.is_ok());
        assert!(rootfs.join("app/app.txt").exists());
        assert_eq!(fs::read_to_string(rootfs.join("app/app.txt")).unwrap(), "application data");
    }

    #[test]
    fn test_execute_copy_directory() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(context.join("src")).unwrap();
        fs::create_dir_all(&rootfs).unwrap();
        fs::write(context.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(context.join("src/lib.rs"), "pub fn hello() {}").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let result = builder.execute_copy(&rootfs, "src", "/app/src");
        assert!(result.is_ok());
        assert!(rootfs.join("app/src/main.rs").exists());
        assert!(rootfs.join("app/src/lib.rs").exists());
    }

    #[test]
    fn test_execute_copy_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&context).unwrap();
        fs::create_dir_all(&rootfs).unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let result = builder.execute_copy(&rootfs, "nonexistent.txt", "/app/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_copy_with_absolute_dest() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&context).unwrap();
        fs::create_dir_all(&rootfs).unwrap();
        fs::write(context.join("config.json"), "{}").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let result = builder.execute_copy(&rootfs, "config.json", "/etc/app/config.json");
        assert!(result.is_ok());
        assert!(rootfs.join("etc/app/config.json").exists());
    }

    #[test]
    fn test_execute_copy_with_relative_dest() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        let rootfs = tmp.path().join("rootfs");

        fs::create_dir_all(&context).unwrap();
        fs::create_dir_all(&rootfs).unwrap();
        fs::write(context.join("data.txt"), "some data").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let result = builder.execute_copy(&rootfs, "data.txt", "app/data.txt");
        assert!(result.is_ok());
        assert!(rootfs.join("app/data.txt").exists());
    }

    #[test]
    fn test_create_layer_tar() {
        let tmp = TempDir::new().unwrap();
        let rootfs = tmp.path().join("rootfs");
        let output = tmp.path().join("layer.tar");

        fs::create_dir_all(&rootfs).unwrap();
        fs::write(rootfs.join("file.txt"), "content").unwrap();
        fs::create_dir_all(rootfs.join("subdir")).unwrap();
        fs::write(rootfs.join("subdir/nested.txt"), "nested content").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let result = builder.create_layer_tar(&rootfs, &output);
        assert!(result.is_ok());
        assert!(output.exists());
        // Verify tar file is not empty
        let metadata = fs::metadata(&output).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    #[serial_test::serial]
    fn test_image_store_root() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", tmp.path());

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let root = builder.image_store_root();
        assert_eq!(root, tmp.path().join("images"));

        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    #[serial_test::serial]
    fn test_image_store_root_default() {
        std::env::remove_var("CUBO_ROOT");

        let tmp = TempDir::new().unwrap();
        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let root = builder.image_store_root();
        assert_eq!(root, PathBuf::from("/var/lib/cubo/images"));
    }

    #[test]
    #[serial_test::serial]
    fn test_save_manifest() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CUBO_ROOT", tmp.path());

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let manifest = ImageManifest {
            reference: "test:v1".to_string(),
            layers: vec!["layer1.tar".to_string()],
            config: ImageConfig {
                cmd: Some(vec!["/bin/sh".to_string()]),
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };

        let result = builder.save_manifest(&manifest);
        assert!(result.is_ok());

        let manifest_path = tmp.path().join("images/manifests/test_v1.json");
        assert!(manifest_path.exists());

        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    fn test_copy_dir_recursive_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("empty_src");
        let dest = tmp.path().join("empty_dest");

        fs::create_dir_all(&src).unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let result = builder.copy_dir_recursive(&src, &dest);
        assert!(result.is_ok());
        assert!(dest.exists());
        assert!(dest.is_dir());
    }

    #[test]
    fn test_copy_dir_recursive_deep_nesting() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        let deep_path = src.join("a/b/c/d/e");
        fs::create_dir_all(&deep_path).unwrap();
        fs::write(deep_path.join("deep.txt"), "deep content").unwrap();

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, tmp.path().to_path_buf());

        let result = builder.copy_dir_recursive(&src, &dest);
        assert!(result.is_ok());
        assert!(dest.join("a/b/c/d/e/deep.txt").exists());
        assert_eq!(fs::read_to_string(dest.join("a/b/c/d/e/deep.txt")).unwrap(), "deep content");
    }

    #[tokio::test]
    async fn test_build_no_base_instruction() {
        let tmp = TempDir::new().unwrap();
        let context = tmp.path().join("context");
        fs::create_dir_all(&context).unwrap();

        let cubofile_content = "RUN echo hello\nCMD /bin/sh";

        let image_store = ImageStore::new(tmp.path().join("images")).unwrap();
        let builder = ImageBuilder::new(&image_store, context);

        let cubofile = crate::container::cubofile::Cubofile::from_string(cubofile_content).unwrap();
        let result = builder.build(&cubofile, "test:build").await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("BASE"));
    }
}
