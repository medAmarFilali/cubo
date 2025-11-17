use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::error::{CuboError, Result};

pub struct ImageStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageManifest {
    /// Image reference (e.g., "ubuntu:latest")
    pub reference: String,
    /// List of layer blob paths
    pub layers: Vec<String>,
    /// Image configuration
    pub config: ImageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Default command to run
    pub cmd: Option<Vec<String>>,
    /// Environment variables
    pub env: Option<Vec<String>>,
    /// Working directory
    pub working_dir: Option<String>,
    /// Exposed ports
    pub exposed_ports: Option<Vec<String>>,
}

impl ImageStore {
    /// Create new image store
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)
            .map_err(|e| CuboError::SystemError(format!("Failed to create image store root: {}", e)))?;
        
        let blobs_dir = root.join("blobs");
        fs::create_dir_all(&blobs_dir)
            .map_err(|e| CuboError::SystemError(format!("Failed to create blobs directory: {}", e)))?;

        let manifests_dir = root.join("manifests");
        fs::create_dir_all(&manifests_dir)
            .map_err(|e| CuboError::SystemError(format!("Failed to create manifests directory: {}", e)))?;

        Ok(Self {root})
    }

    /// Import an image from a tar file
    pub fn import_tar(&self, image_ref: &str, tar_path: &Path) -> Result<()> {
        if !tar_path.exists() {
            return Err(CuboError::SystemError(format!(
                "Image tar file does not exist: {}",
                tar_path.display()
            )))
        }

        let safe_name = image_ref.replace(":", "_");
        let blob_path = self.root.join("blobs").join(format!("{}.tar", safe_name));

        fs::copy(tar_path, &blob_path)
            .map_err(|e| CuboError::SystemError(format!("Failed to copy image tar: {}", e)))?;

        // Create manifest
        let manifest = ImageManifest {
            reference: image_ref.to_string(),
            layers: vec![blob_path.to_string_lossy().to_string()],
            config: ImageConfig {
                cmd: Some(vec!["/bin/sh".to_string()]),
                env: Some(vec!["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()]),
                working_dir: Some("/".to_string()),
                exposed_ports: None,
            }
        };

        self.save_manifest(&manifest)?;
        Ok(())
    }

    pub fn get_manifest(&self, image_ref: &str) -> Result<ImageManifest> {
        let safe_name = image_ref.replace(":", "_");
        let manifest_path = self.root.join("manifests").join(format!("{}.json", safe_name));

        if !manifest_path.exists() {
            return Err(CuboError::BlueprintNotFound(image_ref.to_string()));
        }
        let data = fs::read_to_string(&manifest_path)
            .map_err(|e| CuboError::SystemError(format!("Failed to read manifest file: {}", e)))?;

        let manifest: ImageManifest = serde_json::from_str(&data)
            .map_err(|e| CuboError::SystemError(format!("Failed to parse manifest JSON: {}", e)))?;
        Ok(manifest)
    }

    pub fn has_image(&self, image_ref: &str) -> bool {
        let safe_name = image_ref.replace(":", "_");
        let manifest_path = self.root.join("manifests").join(format!("{}.json", safe_name));
        manifest_path.exists()
    }

    pub fn list_images(&self) -> Result<Vec<String>> {
        let manifests_dir = self.root.join("manifests");
        let mut images = Vec::new();

        if !manifests_dir.exists() {
            return Ok(images);
        }

        for entry in fs::read_dir(&manifests_dir)
            .map_err(|e| CuboError::SystemError(format!("Failed to read manifests dir: {}", e)))?
            {
                let entry = entry
                    .map_err(|e| CuboError::SystemError(format!("Failed to read dir entry: {}", e)))?;
                let path = entry.path();

                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(manifest) = self.get_manifest_by_path(&path) {
                        images.push(manifest.reference);
                    }
                }
            }

        Ok(images)
    }

    pub fn get_layers(&self, image_ref: &str) -> Result<Vec<PathBuf>> {
        let manifest = self.get_manifest(image_ref)?;
        Ok(manifest.layers.iter().map(PathBuf::from).collect())
    }

    pub fn get_config(&self, image_ref: &str) -> Result<ImageConfig> {
        let manifest = self.get_manifest(image_ref)?;
        Ok(manifest.config)
    }


    // Helpers
    fn get_manifest_by_path(&self, path: &Path) -> Result<ImageManifest> {
        let data = fs::read_to_string(path)
            .map_err(|e| CuboError::SystemError(format!("Failed to read manifest file: {}", e)))?;

        let manifest: ImageManifest = serde_json::from_str(&data)
            .map_err(|e| CuboError::SystemError(format!("Failed to parse manifest: {}", e)))?;

        Ok(manifest)
    } 
    fn save_manifest(&self, manifest: &ImageManifest) -> Result<()> {
        let safe_name = manifest.reference.replace(":", "_");
        let manifest_path = self.root.join("manifests").join(format!("{}.json", safe_name));

        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| CuboError::SystemError(format!("Failed to write manifest: {}", e)))?;

        fs::write(&manifest_path, json)
            .map_err(|e| CuboError::SystemError(format!("Failed to write manifest file:: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_image_store_creation() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf()).unwrap();

        assert!(tmp.path().join("blobs").exists());
        assert!(tmp.path().join("manifests").exists());
    }

    #[test]
    fn test_manifest_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = ImageManifest {
            reference: "alpine:latest".to_string(),
            layers: vec!["/path/to/layer.tar".to_string()],
            config: ImageConfig {
                cmd: Some(vec!["/bin/sh".to_string()]),
                env: None,
                working_dir: Some("/".to_string()),
                exposed_ports: None,
            },
        };

        store.save_manifest(&manifest).unwrap();

        let loaded = store.get_manifest("alpine:latest").unwrap();
        assert_eq!(loaded.reference, "alpine:latest");
        assert_eq!(loaded.layers.len(), 1);
    }

    #[test]
    fn test_has_image() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf()).unwrap();

        assert!(!store.has_image("alpine:latest"));

        let manifest = ImageManifest {
            reference: "alpine:latest".to_string(),
            layers: vec![],
            config: ImageConfig {
                cmd: None,
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };

        store.save_manifest(&manifest).unwrap();
        assert!(store.has_image("alpine:latest"));
    }
}
