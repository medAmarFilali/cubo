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