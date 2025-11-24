use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CuboError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CubofileToml {
    /// Base image configuration
    pub image: ImageSpec,
    /// Run instructions
    #[serde(default)]
    pub run: Vec<RunStep>,
    /// COPY instructions
    #[serde(default)]
    pub copy: Vec<CopyStep>,
    /// Image configuration line env, workdir, cmd
    #[serde(default)]
    pub config: Config,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSpec {
    /// Base image
    pub base: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunStep {
    /// Command to execute
    pub command: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CopyStep {
    /// Source path in build context
    pub src: String,
    /// Destination path in container
    pub dest: String,
}

/// Container config
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory
    pub workdir: Option<String>,
    /// Default command
    pub cmd: Option<Vec<String>>,
    
    /// Exposed ports
    #[serde(default)]
    pub expose: Vec<String>,
}

impl CubofileToml {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .map_err(|e| CuboError::SystemError(format!("Failed to read Cubofile.toml: {}", e)))?;

        Self::from_string(&content)
    }

    pub fn from_string(content: &str) -> Result<Self> {
        toml::from_str(content)
            .map_err(|e| CuboError::SystemError(format!("Failed to parse Cubofile.toml: {}", e)))
    }

    pub fn base_image(&self) -> String {
        self.image.base.clone()
    }

    pub fn run_commands(&self) -> Vec<String> {
        self.run.iter().map(|r| r.command.clone()).collect()
    }

    pub fn copy_steps(&self) -> Vec<(String, String)> {
        self.copy.iter().map(|c| (c.src.clone(), c.dest.clone())).collect()
    }
    

}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_minimal_cubofile_toml() {
        let content = r#"
[image]
base = "alpine:latest"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.base_image(), "alpine:latest");
        assert_eq!(cubofile.run.len(), 0);
        assert_eq!(cubofile.copy.len(), 0);
    }

    #[test]
    fn test_parse_full_cubofile_toml() {
        let content = r#"
[image]
base = "alpine:latest"

[[run]]
command = "apk add --no-cache curl"

[[run]]
command = "apk add git"

[[copy]]
src = "./myapp"
dest = "/usr/local/bin/myapp"

[[copy]]
src = "./config.toml"
dest = "/etc/app/config.toml"

[config]
workdir = "/app"
cmd = ["/usr/local/bin/myapp", "serve"]
expose = ["8080", "9090"]

[config.env]
PATH = "/usr/local/bin:/usr/bin:/bin"
APP_ENV = "production"
LOG_LEVEL = "info"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();

        assert_eq!(cubofile.base_image(), "alpine:latest");
        assert_eq!(cubofile.run.len(), 2);
        assert_eq!(cubofile.run[0].command, "apk add --no-cache curl");
        assert_eq!(cubofile.copy.len(), 2);
        assert_eq!(cubofile.copy[0].src, "./myapp");
        assert_eq!(cubofile.copy[0].dest, "/usr/local/bin/myapp");
        assert_eq!(cubofile.config.workdir, Some("/app".to_string()));
        assert_eq!(cubofile.config.cmd, Some(vec!["/usr/local/bin/myapp".to_string(), "serve".to_string()]));
        assert_eq!(cubofile.config.env.get("APP_ENV"), Some(&"production".to_string()));
        assert_eq!(cubofile.config.expose.len(), 2);
    }

    #[test]
    fn test_parse_with_only_run() {
        let content = r#"
[image]
base = "ubuntu:20.04"

[[run]]
command = "apt-get update"

[[run]]
command = "apt-get install -y curl"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.run.len(), 2);
        assert_eq!(cubofile.run_commands()[0], "apt-get update");
    }

    #[test]
    fn test_parse_with_config_only() {
        let content = r#"
[image]
base = "alpine:latest"

[config]
workdir = "/workspace"
cmd = ["/bin/sh"]

[config.env]
HOME = "/root"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.config.workdir, Some("/workspace".to_string()));
        assert_eq!(cubofile.config.env.get("HOME"), Some(&"/root".to_string()));
    }

    #[test]
    fn test_invalid_toml() {
        let content = "invalid toml {{{";
        let result = CubofileToml::from_string(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_base() {
        let content = r#"
[image]
# Missing base field

[[run]]
command = "echo hello"
"#;

        let result = CubofileToml::from_string(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("Cubofile.toml");

        let content = r#"
[image]
base = "nginx:latest"

[[run]]
command = "nginx -t"
"#;
        fs::write(&file_path, content).unwrap();

        let cubofile = CubofileToml::from_file(&file_path).unwrap();
        assert_eq!(cubofile.base_image(), "nginx:latest");
        assert_eq!(cubofile.run.len(), 1);
    }

    #[test]
    fn test_from_file_not_found() {
        let result = CubofileToml::from_file(Path::new("/nonexistent/Cubofile.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_run_commands() {
        let content = r#"
[image]
base = "alpine:latest"

[[run]]
command = "echo 1"

[[run]]
command = "echo 2"

[[run]]
command = "echo 3"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        let commands = cubofile.run_commands();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0], "echo 1");
        assert_eq!(commands[1], "echo 2");
        assert_eq!(commands[2], "echo 3");
    }

    #[test]
    fn test_copy_steps() {
        let content = r#"
[image]
base = "alpine:latest"

[[copy]]
src = "./app"
dest = "/app"

[[copy]]
src = "./config.json"
dest = "/etc/config.json"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        let steps = cubofile.copy_steps();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0], ("./app".to_string(), "/app".to_string()));
        assert_eq!(steps[1], ("./config.json".to_string(), "/etc/config.json".to_string()));
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.env.is_empty());
        assert!(config.workdir.is_none());
        assert!(config.cmd.is_none());
        assert!(config.expose.is_empty());
    }

    #[test]
    fn test_empty_run_and_copy() {
        let content = r#"
[image]
base = "scratch"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.run_commands().len(), 0);
        assert_eq!(cubofile.copy_steps().len(), 0);
    }

    #[test]
    fn test_serialization() {
        let cubofile = CubofileToml {
            image: ImageSpec {
                base: "alpine:3.18".to_string(),
            },
            run: vec![RunStep {
                command: "echo hello".to_string(),
            }],
            copy: vec![CopyStep {
                src: "./src".to_string(),
                dest: "/app/src".to_string(),
            }],
            config: Config {
                env: HashMap::from([("KEY".to_string(), "value".to_string())]),
                workdir: Some("/app".to_string()),
                cmd: Some(vec!["/app/start".to_string()]),
                expose: vec!["8080".to_string()],
            },
        };

        let toml_str = toml::to_string(&cubofile).unwrap();
        assert!(toml_str.contains("alpine:3.18"));
        assert!(toml_str.contains("echo hello"));

        let parsed: CubofileToml = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.base_image(), "alpine:3.18");
    }

    #[test]
    fn test_clone() {
        let cubofile = CubofileToml {
            image: ImageSpec {
                base: "ubuntu:22.04".to_string(),
            },
            run: vec![RunStep {
                command: "apt update".to_string(),
            }],
            copy: vec![],
            config: Config::default(),
        };

        let cloned = cubofile.clone();
        assert_eq!(cloned.base_image(), "ubuntu:22.04");
        assert_eq!(cloned.run.len(), 1);
    }

    #[test]
    fn test_debug_trait() {
        let cubofile = CubofileToml {
            image: ImageSpec {
                base: "alpine:latest".to_string(),
            },
            run: vec![],
            copy: vec![],
            config: Config::default(),
        };

        let debug_str = format!("{:?}", cubofile);
        assert!(debug_str.contains("CubofileToml"));
        assert!(debug_str.contains("alpine:latest"));
    }

    #[test]
    fn test_with_empty_env() {
        let content = r#"
[image]
base = "alpine:latest"

[config]
workdir = "/app"
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert!(cubofile.config.env.is_empty());
        assert_eq!(cubofile.config.workdir, Some("/app".to_string()));
    }

    #[test]
    fn test_with_single_cmd_element() {
        let content = r#"
[image]
base = "alpine:latest"

[config]
cmd = ["/bin/sh"]
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.config.cmd, Some(vec!["/bin/sh".to_string()]));
    }

    #[test]
    fn test_with_many_exposed_ports() {
        let content = r#"
[image]
base = "alpine:latest"

[config]
expose = ["80", "443", "8080", "9000"]
"#;

        let cubofile = CubofileToml::from_string(content).unwrap();
        assert_eq!(cubofile.config.expose.len(), 4);
        assert!(cubofile.config.expose.contains(&"80".to_string()));
        assert!(cubofile.config.expose.contains(&"443".to_string()));
    }

    #[test]
    fn test_run_step_clone_and_debug() {
        let step = RunStep {
            command: "test command".to_string(),
        };
        let cloned = step.clone();
        assert_eq!(cloned.command, "test command");

        let debug_str = format!("{:?}", step);
        assert!(debug_str.contains("RunStep"));
    }

    #[test]
    fn test_copy_step_clone_and_debug() {
        let step = CopyStep {
            src: "./source".to_string(),
            dest: "/dest".to_string(),
        };
        let cloned = step.clone();
        assert_eq!(cloned.src, "./source");
        assert_eq!(cloned.dest, "/dest");

        let debug_str = format!("{:?}", step);
        assert!(debug_str.contains("CopyStep"));
    }

    #[test]
    fn test_image_spec_clone_and_debug() {
        let spec = ImageSpec {
            base: "debian:bullseye".to_string(),
        };
        let cloned = spec.clone();
        assert_eq!(cloned.base, "debian:bullseye");

        let debug_str = format!("{:?}", spec);
        assert!(debug_str.contains("ImageSpec"));
    }
}