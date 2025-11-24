use tempfile::TempDir;

use cubo::container::{Container, ContainerStatus};
use cubo::container::runtime::{ContainerRuntime, RuntimeConfig};
use cubo::container::image_store::{ImageStore, ImageManifest, ImageConfig};
use cubo::container::cubofile::Cubofile;
use cubo::container::cubofile_toml::CubofileToml;

fn create_test_runtime() -> (ContainerRuntime, TempDir) {
    let temp = TempDir::new().unwrap();
    let config = RuntimeConfig {
        root_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let runtime = ContainerRuntime::new(config).unwrap();
    (runtime, temp)
}

// Lifecycle tests
#[tokio::test]
async fn test_container_lifecycle() {
    let (runtime, _temp_dir) = create_test_runtime();

    // Create container
    let container = Container::new(
        "test:lifecycle".to_string(),
        vec!["echo".to_string(), "hello world!!".to_string()],
    ).with_name("lifecycle-test".to_string());
    let container_id = runtime.create_container(container).await.unwrap();
    assert!(!container_id.is_empty());

    // Check container exists
    let retrieved = runtime.get_container(&container_id).await.unwrap();
    assert_eq!(retrieved.status, ContainerStatus::Created);
    assert_eq!(retrieved.name, Some("lifecycle-test".to_string()));

    // list all containers
    let all_containers = runtime.list_containers(true).await.unwrap();
    assert_eq!(all_containers.len(), 1);

    // List running containers (should be empty since we haven't started)
    let running_containers = runtime.list_containers(false).await.unwrap();
    assert!(running_containers.is_empty());

    // Remove container
    runtime.remove_container(&container_id, false).await.unwrap();

    // Check container is nada
    let result = runtime.get_container(&container_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_containers_isolation() {
    let (runtime, _temp_path) = create_test_runtime();

    let mut container_ids = Vec::new();
    for i in 0..5 {
        let container = Container::new(
            format!("test:v{}", i),
            vec!["echo".to_string(), format!("{}", i)]
        ).with_name(format!("container-{}", i));
        let id = runtime.create_container(container).await.unwrap();
        container_ids.push(id);
    }

    // Check all containers exist
    let all = runtime.list_containers(true).await.unwrap();
    assert_eq!(all.len(), 5);

    // Check each has unique IDs
    let unique_ids: std::collections::HashSet<_> = container_ids.iter().collect();
    assert_eq!(unique_ids.len(), 5);

    // Remove all containers
    for id in &container_ids {
        runtime.remove_container(id, false).await.unwrap();
    }

    let remaining = runtime.list_containers(true).await.unwrap();
    assert!(remaining.is_empty());
}

// Image store tests]
#[test]
fn test_image_store_import_and_retrieve() {
    let temp_dir = TempDir::new().unwrap();
    let store = ImageStore::new(temp_dir.path().to_path_buf()).unwrap();

    // save manifest
    let manifest = ImageManifest {
        reference: "integration:test".to_string(),
        layers: vec!["layer1.tar".to_string()],
        config: ImageConfig {
            cmd: Some(vec!["/bin/sh".to_string()]),
            env: Some(vec!["PATH=/bin".to_string()]),
            working_dir: Some("/".to_string()),
            exposed_ports: None,
        },
    };

    store.save_manifest(&manifest).unwrap();

    // Check if we can retrieve it
    assert!(store.has_image("integration:test"));

    let retrieved = store.get_manifest("integration:test").unwrap();
    assert_eq!(retrieved.reference, "integration:test");
    assert_eq!(retrieved.layers.len(), 1);

    let config = store.get_config("integration:test").unwrap();
    assert_eq!(config.cmd, Some(vec!["/bin/sh".to_string()]));
}

#[test]
fn test_image_store_list_multiple() {
    let temp_dir = TempDir::new().unwrap();
    let store = ImageStore::new(temp_dir.path().to_path_buf()).unwrap();

    let images = vec![
        "alpine:latest",
        "ubuntu:22.04",
        "nginx:1.25",
        "redis:7",
        "postgres:15",
    ];

    for img in &images {
        let manifest = ImageManifest {
            reference: img.to_string(),
            layers: vec![],
            config: ImageConfig {
                cmd: None,
                env: None,
                working_dir: None,
                exposed_ports: None,
            },
        };
        store.save_manifest(&manifest).unwrap();
    }
    let listed = store.list_images().unwrap();
    assert_eq!(listed.len(), 5);
    for img in &images {
        assert!(store.has_image(img));
    }
}


// Cubofile Parsing Tests

#[test]
fn test_cubofile_parsing_integration() {
    let content = r#"
# This is a comment
BASE alpine:3.18

# Install dependencies
RUN apk add --no-cache curl wget

# Copy application fils
COPY ./src /app/src
COPY ./config.json /app/config.json

# Set environment
ENV APP_ENV=production
ENV DEBUG=false

# Set working directory
WORKDIR /app

# Default command
CMD /app/start.sh
"#;

    let cubofile = Cubofile::from_string(content).unwrap();

    assert_eq!(cubofile.base_image(), Some("alpine:3.18".to_string()));

    let run_commands = cubofile.run_commands();
    assert_eq!(run_commands.len(), 1);
    assert!(run_commands[0].contains("apk add"));
}

#[test]
fn test_cubofile_toml_parsing_integration() {
    let content = r#"
[image]
base = "alpine:3.18"

[[run]]
command = "apk add --no-cache curl"

[[run]]
command = "mkdir -p /app"

[[copy]]
src = "./src"
dest = "/app/src"

[config]
cmd = ["/app/start.sh"]
workdir = "/app"
expose = ["8080", "443"]

[config.env]
APP_ENV = "production"
LOG_LEVEL = "info"
"#;

    let cubofile = CubofileToml::from_string(content).unwrap();

    assert_eq!(cubofile.base_image(), "alpine:3.18");
    assert_eq!(cubofile.run.len(), 2);
    assert_eq!(cubofile.copy.len(), 1);

    let config = &cubofile.config;
    assert_eq!(config.workdir, Some("/app".to_string()));
    assert_eq!(config.env.len(), 2);
}

// Container configuration tests

#[tokio::test]
async fn test_cotnainer_with_environment_variables() {
    let (runtime, _temp_dir) = create_test_runtime();
    let mut container = Container::new(
        "test:env".to_string(),
        vec!["printenv".to_string()],
    );
    container = container.with_env("FOO".to_string(), "bar".to_string());
    container = container.with_env("BAZ".to_string(), "qux".to_string());
    let container_id = runtime.create_container(container).await.unwrap();
    let retrieved = runtime.get_container(&container_id).await.unwrap();
    assert_eq!(retrieved.config.env_vars.get("FOO"), Some(&"bar".to_string()));
    assert_eq!(retrieved.config.env_vars.get("BAZ"), Some(&"qux".to_string()));
}

#[tokio::test]
async fn test_contaienr_with_working_directory() {
    let (runtime, _temp_dir) = create_test_runtime();
    let container = Container::new(
        "test:workdir".to_string(),
        vec!["pwd".to_string()],
    ).with_workdir("/app/src".to_string());
    let container_id = runtime.create_container(container).await.unwrap();
    let retrieved = runtime.get_container(&container_id).await.unwrap();
    assert_eq!(retrieved.config.working_dir, Some("/app/src".to_string()));
}


// Persistence tests
#[tokio::test]
async fn test_container_persistence_across_restarts() {
    let temp_dir = TempDir::new().unwrap();
    let root_path = temp_dir.path().to_path_buf();
    let container_id: String;

    // Runtime instance - create container
    {
        let config = RuntimeConfig{
            root_dir: root_path.clone(),
            ..Default::default()
        };

        let runtime = ContainerRuntime::new(config).unwrap();
        let container = Container::new(
            "persist:test".to_string(),
            vec!["echo".to_string()]
        ).with_name("persistent-container".to_string())
        .with_workdir("/app".to_string())
        .with_env("KEY".to_string(), "value".to_string());
        container_id = runtime.create_container(container).await.unwrap();
    }

    // runtime instance - check container presisted
    {
        let config = RuntimeConfig {
            root_dir: root_path.clone(),
            ..Default::default()
        };
        let runtime = ContainerRuntime::new(config).unwrap();
        let loaded = runtime.get_container(&container_id).await.unwrap();
        assert_eq!(loaded.blueprint, "persist:test");
        assert_eq!(loaded.name, Some("persistent-container".to_string()));
        assert_eq!(loaded.config.working_dir, Some("/app".to_string()));
        assert_eq!(loaded.config.env_vars.get("KEY"), Some(&"value".to_string()));
    }
}

// Error handling tests
#[tokio::test]
async fn test_get_nonexistent_container() {
    let (runtime, _temp_dir) = create_test_runtime();
    let result = runtime.get_container("nonexistent-container-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_nonexistent_container() {
    let (runtime, _temp_dir) = create_test_runtime();
    let result = runtime.remove_container("nonexistent-container-id", false).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stop_nonexistent_container() {
    let (runtime, _temp_dir) = create_test_runtime();
    let result = runtime.stop_container("nonexistent-container-id", None).await;
    assert!(result.is_err());
}



// Builder pattern tests
#[test]
fn test_container_builder_pattern() {
    let container = Container::new(
        "builder:test".to_string(),
        vec!["command".to_string()]
    )
    .with_name("builder-test".to_string())
    .with_workdir("/app".to_string())
    .with_env("ENV1".to_string(), "value1".to_string())
    .with_env("ENV2".to_string(), "value2".to_string());

    assert_eq!(container.name, Some("builder-test".to_string()));
    assert_eq!(container.config.working_dir, Some("/app".to_string()));
    assert_eq!(container.config.env_vars.len(), 2);
}