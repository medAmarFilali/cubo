use std::fs;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use oci_distribution::client::{Client, ClientConfig, ClientProtocol};
use oci_distribution::Reference;
use tracing::{info, debug};
use serde::{Deserialize, Serialize};

use crate::error::{CuboError, Result};
use super::image_store::{ImageStore, ImageManifest, ImageConfig};


#[derive(Debug, Deserialize, Serialize)]
struct OciManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: i32,
    #[serde(rename = "mediaType")]
    media_type: Option<String>,
    config: OciDescriptor,
    layers: Vec<OciDescriptor>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ManifestList {
    #[serde(rename = "schemaVersion")]
    schema_version: i32,
    #[serde(rename = "mediaType")]
    media_type: String,
    manifests: Vec<ManifestDescriptor>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ManifestDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: i64,
    platform: Option<Platform>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Platform {
    architecture: String,
    os: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OciDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    size: i64,
    digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OciImageConfig {
    config: Option<OciConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OciConfig {
    #[serde(rename = "Env")]
    env: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    cmd: Option<Vec<String>>,
    #[serde(rename = "WorkingDir")]
    working_dir: Option<String>,
    #[serde(rename = "ExposedPorts")]
    exposed_ports: Option<serde_json::Value>,
}

/// client
pub struct RegistryClient {
    client: Client,
    image_store: ImageStore,
}

impl RegistryClient {
    pub fn new(image_store: ImageStore) -> Self {
        let config = ClientConfig {
            protocol: ClientProtocol::Https,
            ..Default::default()
        };

        Self {
            client: Client::new(config),
            image_store,
        }
    }

    pub async fn pull(&self, image_ref: &str) -> Result<()> {
        info!("Pulling image: {}", image_ref);
        if self.image_store.has_image(image_ref) {
            info!("Image {} already exists locally", image_ref);
            return Ok(());
        }

        let (registry, repository, tag ) = Self::parse_image_ref(image_ref)?;
        info!("Registry: {}, Repository: {}, tag: {}", registry, repository, tag);

        let http_client = reqwest::Client::builder()
            .user_agent("cubo/0.1.0")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| CuboError::SystemError(format!("Failed to create http client: {}", e)))?;
        let token = Self::get_registry_token(&http_client, &registry, &repository).await?;
        info!("Fetching manifest...");
        let manifest = Self::fetch_manifest(&http_client, &registry, &repository, &tag, &token).await?;
        info!("Manifest fetched: {} layers", manifest.layers.len());
        info!("Fetching image config...");
        let config_data = Self::fetch_blob(&http_client, &registry, &repository, &manifest.config.digest, &token).await?;
        let oci_config: OciImageConfig = serde_json::from_slice(&config_data)
            .map_err(|e| CuboError::SystemError(format!("Failed to parse image config: {}", e)))?;

        let temp_dir = tempfile::tempdir()
            .map_err(|e| CuboError::SystemError(format!("Failed to create temp dir: {}", e)))?;

        let mut layer_paths = Vec::new();
        for (idx, layer_desc) in manifest.layers.iter().enumerate() {
            info!("Downloading layer {}/{} ({})", idx + 1, manifest.layers.len(), layer_desc.media_type);

            let layer_data = Self::fetch_blob(&http_client, &registry, &repository, &layer_desc.digest, &token).await?;

            let layer_file = temp_dir.path().join(format!("layer_{}.blob", idx));
            fs::write(&layer_file, &layer_data)
                .map_err(|e| CuboError::SystemError(format!("Failed to write layer: {}", e)))?;
            let final_layer = if Self::is_gzipped(&layer_data) {
                let decompressed_path = temp_dir.path().join(format!("layer_{}.tar", idx));
                Self::decompress_gzip(&layer_file, &decompressed_path)?;
                decompressed_path
            } else {
                layer_file
            };
            let safe_name = image_ref.replace(':', "_").replace('/', "_");
            let blob_path = self
                .image_store_root()
                .join("blobs")
                .join(format!("{}_{}.tar", safe_name, idx));

            fs::create_dir_all(blob_path.parent().unwrap()).map_err(|e| {
                CuboError::SystemError(format!("Failed to create blobs directoy: {}", e))
            })?;

            fs::copy(&final_layer, &blob_path)
                .map_err(|e| CuboError::SystemError(format!("Failed to copy layer: {}", e)))?;
            layer_paths.push(blob_path.to_string_lossy().to_string());
        }

        let image_config = Self::convert_oci_config(&oci_config);
        let manifest_obj = ImageManifest {
            reference: image_ref.to_string(),
            layers: layer_paths,
            config: image_config,
        };
        self.save_manifest(&manifest_obj)?;
        info!("Successfully pulled and stored image: {}", image_ref);
        Ok(())
    }

    fn parse_image_ref(image_ref: &str) -> Result<(String, String, String)> {
        let parts: Vec<&str> = image_ref.split(':').collect();
        let (image_path, tag) = if parts.len() == 2 {
            (parts[0], parts[1].to_string())
        } else {
            (image_ref, "latest".to_string())
        };

        let (registry, repository) = if image_path.contains('/') {
            let path_parts: Vec<&str> = image_path.splitn(2, '/').collect();
            if path_parts[0].contains('.') || path_parts[0] == "localhost" {
                (path_parts[0].to_string(), path_parts[1].to_string())
            } else {
                ("registry-1.docker.io".to_string(), image_path.to_string())
            }
        } else {
            ("registry-1.docker.io".to_string(), format!("library/{}", image_path))
        };

        Ok((registry, repository, tag))
    }

    async fn get_registry_token(client: &reqwest::Client, registry: &str, repository: &str) -> Result<String> {
        if registry == "registry-1.docker.io" {
            let url = format!(
                "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
                repository
            );

            let response = client.get(&url)
                .send()
                .await
                .map_err(|e| CuboError::SystemError(format!("Failed to get auth token: {}", e)))?;

            if !response.status().is_success() {
                return Err(CuboError::SystemError(format!(
                    "Failed to get auth token: HTTP {}",
                    response.status()
                )));
            }

            #[derive(Deserialize)]
            struct TokenResponse {
                token: String,
            }

            let token_res: TokenResponse = response.json().await
                .map_err(|e| CuboError::SystemError(format!("Failed to parse token response: {}", e)))?;

            Ok(token_res.token)
        } else {
            Ok(String::new())
        }
    }

    async fn fetch_manifest(
        client: &reqwest::Client,
        registry: &str,
        repository: &str,
        tag: &str,
        token: &str,
    ) -> Result<OciManifest> {
        let url = format!("https://{}/v2/{}/manifests/{}", registry, repository, tag);    
        let mut request = client.get(&url);
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }

        request = request.header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, \
             application/vnd.docker.distribution.manifest.list.v2+json, \
             application/vnd.oci.image.manifest.v1+json, \
             application/vnd.oci.image.index.v1+json",
        );

        let response = request
            .send()
            .await
            .map_err(|e| CuboError::SystemError(format!("Failed to fetch manifest: {}", e)))?;

            if !response.status().is_success() {
            return Err(CuboError::SystemError(format!(
                "Failed to fetch manifest: HTTP {}",
                response.status()
            )));
        }

        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let response_text = response.text().await
            .map_err(|e| CuboError::SystemError(format!("Failed to read response: {}", e)))?;

        if content_type.contains("manifest.list") || content_type.contains("image.index") {
            debug!("Received manifest list, selecting platform-specific manifest");
            let manifest_list: ManifestList = serde_json::from_str(&response_text)
                .map_err(|e| CuboError::SystemError(format!("Failed to parse manifest list: {}", e)))?;

            let platform_manifest = manifest_list.manifests.iter()
                .find(|m| {
                    m.platform.as_ref()
                        .map(|p| p.os == "linux" && p.architecture == "amd64")
                        .unwrap_or(false)
                })
                .or_else(|| manifest_list.manifests.first())
                .ok_or_else(|| CuboError::SystemError("No suitable manifest found in list".to_string()))?;

            info!("Selected manifest for platform: linux/amd64");

            Self::fetch_manifest_by_digest(client, registry, repository, &platform_manifest.digest, token).await
        } else {
            let manifest: OciManifest = serde_json::from_str(&response_text)
                .map_err(|e| CuboError::SystemError(format!("Failed to parse manifest: {}", e)))?;
            Ok(manifest)
        }
    }

    async fn fetch_manifest_by_digest(
        client: &reqwest::Client,
        registry: &str,
        repository: &str,
        digest: &str,
        token: &str,
    ) -> Result<OciManifest> {
        let url = format!("https://{}/v2/{}/manifests/{}", registry, repository, digest);

        let mut request = client.get(&url);

        if !token.is_empty() {
            request = request.bearer_auth(token);
        }

        request = request.header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json",
        );

        let response = request
            .send()
            .await
            .map_err(|e| CuboError::SystemError(format!("Failed to fetch manifest by digest: {}", e)))?;

        if !response.status().is_success() {
            return Err(CuboError::SystemError(format!(
                "Failed to fetch manifest: HTTP {}",
                response.status()
            )));
        }

        let manifest: OciManifest = response.json().await
            .map_err(|e| CuboError::SystemError(format!("Failed to parse manifest: {}", e)))?;

        Ok(manifest)
    }

    async fn fetch_blob(
        client: &reqwest::Client,
        registry: &str,
        repository: &str,
        digest: &str,
        token: &str,
    ) -> Result<Vec<u8>> {
        let url = format!("https://{}/v2/{}/blobs/{}", registry, repository, digest);
        let mut request = client.get(&url);

        if !token.is_empty() {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| CuboError::SystemError(format!("Fialed to fetch blob: {}: {}", digest, e)))?;

        if !response.status().is_success() {
            return Err(CuboError::SystemError(format!(
                "Failed to fetch blob {}: HTTP {}",
                digest,
                response.status()
            )));
        }

        let data = response.bytes().await
            .map_err(|e| CuboError::SystemError(format!("Failed to read blob data: {}", e)))?
            .to_vec();

        Ok(data)
    }

    fn convert_oci_config(oci_config: &OciImageConfig) -> ImageConfig {
        let config = oci_config.config.as_ref();

        ImageConfig {
            cmd: config.and_then(|c| c.cmd.clone()),
            env: config.and_then(|c| c.env.clone()),
            working_dir: config.and_then(|c| c.working_dir.clone()),
            exposed_ports: config.and_then(|c| {
                c.exposed_ports.as_ref().and_then(|ports| {
                    if let serde_json::Value::Object(map) = ports {
                        Some(map.keys().cloned().collect())
                    } else {
                        None
                    }
                })
            })
        }
    }
 
    fn save_manifest(&self, manifest: &ImageManifest) -> Result<()> {
        let safe_name = manifest.reference.replace(':', "_");
        let manifest_path = self
            .image_store_root()
            .join("manifests")
            .join(format!("{}.json", safe_name));
        fs::create_dir_all(manifest_path.parent().unwrap()).map_err(|e| {
            CuboError::SystemError(format!("Failed to create manifests dir: {}", e))
        })?;
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| CuboError::SystemError(format!("Failed to serialize manifest: {}", e)))?;
        fs::write(&manifest_path, json)
            .map_err(|e| CuboError::SystemError(format!("Failed to write manifest: {}", e)))?;
        Ok(())
    }

    fn parse_image_config(_config_data: &oci_distribution::client::Config) -> Result<ImageConfig> {
        Ok(ImageConfig {
            cmd: Some(vec!["/bin/sh".to_string()]),
            env: Some(vec!["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()]),
            working_dir: Some("/".to_string()),
            exposed_ports: None,
        })
    } 

    fn is_gzipped(data: &[u8]) -> bool {
        data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
    }

    fn decompress_gzip(input: &Path, output: &Path) -> Result<()> {
        let input_file = fs::File::open(input)
            .map_err(|e| CuboError::SystemError(format!("Failed to open gzip file: {}", e)))?;

        let mut decoder = GzDecoder::new(input_file);
        let mut output_file = fs::File::create(output)
            .map_err(|e| CuboError::SystemError(format!("Failed to create output file: {}", e)))?;

        std::io::copy(&mut decoder, &mut output_file)
            .map_err(|e| CuboError::SystemError(format!("Failed to decompress gzip: {}", e)))?;

        Ok(())
    }

    fn parse_reference(image_ref: &str) -> Result<Reference> {
        let full_ref = if !image_ref.contains('/') {
            format!("docker.io/library/{}", image_ref)
        } else if !image_ref.starts_with("docker.io") && !image_ref.contains('.') {
            format!("docker.io/{}", image_ref)
        } else {
            image_ref.to_string()
        };

        let full_ref = if !full_ref.contains(':') && !full_ref.contains('@') {
            format!("{}:latest", full_ref)
        } else {
            full_ref
        };

        Reference::try_from(full_ref.as_str()).map_err(|e| {
            CuboError::InvalidConfiguration(format!("Invalid image reference '{}': {}", image_ref, e))
        })
    }

    fn image_store_root(&self) -> PathBuf {
        std::env::var("CUBO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/var/lib/cubo"))
            .join("images")
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reference_short() {
        let ref_str = RegistryClient::parse_reference("alpine")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("docker.io/library/alpine:latest"));
    }

    #[test]
    fn test_parse_reference_with_tag() {
        let ref_str = RegistryClient::parse_reference("alpine:3.18")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("alpine:3.18"));
    }

    #[test]
    fn test_parse_reference_user_image() {
        let ref_str = RegistryClient::parse_reference("user/theimage")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("docker.io/user/theimage:latest"));
    }

    #[test]
    fn test_is_gzipped() {
        let gzip_magic = vec![0x1f, 0x8b, 0x08, 0x00];
        assert!(RegistryClient::is_gzipped(&gzip_magic));

        let not_gzip = vec![0x00, 0x00, 0x00, 0x00];
        assert!(!RegistryClient::is_gzipped(&not_gzip));
    }

    #[test]
    fn test_is_gzipped_empty_data() {
        let empty = vec![];
        assert!(!RegistryClient::is_gzipped(&empty));
    }

    #[test]
    fn test_is_gzipped_single_byte() {
        let single = vec![0x1f];
        assert!(!RegistryClient::is_gzipped(&single));
    }

    #[test]
    fn test_parse_reference_ghcr() {
        let ref_str = RegistryClient::parse_reference("ghcr.io/owner/repo:v1.0")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("ghcr.io"));
        assert!(ref_str.contains("owner/repo"));
        assert!(ref_str.contains("v1.0"));
    }

    #[test]
    fn test_parase_reference_gcr() {
        let ref_str = RegistryClient::parse_reference("gcr.io/project/image:latest")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("gcr.io"));
    }

    #[test]
    fn test_parse_reference_quay() {
        let ref_str = RegistryClient::parse_reference("quay.io/organization/image:1.0")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("quay.io"));
    }

    #[test]
    fn test_parse_reference_docker_io_explicit() {
        let ref_str = RegistryClient::parse_reference("docker.io/library/nginx:1.25")
            .unwrap()
            .to_string();
        assert!(ref_str.contains("nginx"));
        assert!(ref_str.contains("1.25"));
    }

    #[test]
    fn test_registry_client_creation() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let store = crate::container::image_store::ImageStore::new(tmp.path().to_path_buf()).unwrap();
        let _client = RegistryClient::new(store);
    }
    
    #[test]
    fn test_image_manifest_serialization() {
        use crate::container::image_store::{ImageConfig, ImageManifest};

        let manifest = ImageManifest {
            reference: "test:latest".to_string(),
            layers: vec!["layer1.tar".to_string(), "layer2.tar".to_string()],
            config: ImageConfig {
                cmd: Some(vec!["/bin/bash".to_string()]),
                env: Some(vec!["PATH=/bin".to_string()]),
                working_dir: Some("/app".to_string()),
                exposed_ports: Some(vec!["80/tcp".to_string()]),
            },
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("test:latest"));
        assert!(json.contains("layer1.tar"));
        let deserialized: ImageManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.reference, "test:latest");
        assert_eq!(deserialized.layers.len(), 2);
    }
}