use clap::{Parser, Subcommand};


#[derive(Parser)]
#[command(name = "cubo", version = "0.1.0", author = "Amar FILALI", about = "A lightweight containerization tool focused on isolation and security.")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true, env = "CUBO_ROOT", value_name = "PATH")]
    pub root_dir: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Runs the container from a specified blueprint file.
    Run(RunArgs),
    /// Build a blueprint from a Cubofile.
    Build(BuildArgs),
    /// List running containers
    Ps(PsArgs),
    /// List Blueprints
    Blueprint(BlueprintArgs),
    /// Stop a running container
    Stop(StopArgs),
    /// Remove containers
    Rm(RmArgs),
    // Remove blueprints
    // rmb(RmbArgs),
    /// Pull an image from a registry
    Pull(PullArgs)
}

#[derive(Debug, Parser)]
pub struct RunArgs {
    /// Blueprint name or ID
    pub blueprint: String,
    /// Command to run inside the container
    pub command: Option<Vec<String>>,
    /// name of the container
    #[arg(short, long)]
    pub name: Option<String>,
    /// Run in detached mode
    #[arg(short, long)]
    pub detach: bool,
    /// Bind mount a volume (host->container)
    #[arg(short,long)]
    pub volume: Vec<String>,
    /// Publish ports (host->container)
    #[arg(short, long)]
    pub publish : Vec<String>,
    /// Environment variables
    #[arg(short, long)]
    pub env: Vec<String>,
    /// Working directory
    pub workdir: Option<String>,
}

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// Path to build context
    pub path: String,
    /// Name and optionally tag (name->tag)
    pub tag: Option<String>,
    // Path to the build file auto-detectes Cubofile.toml or Cubofile if not specified
    #[arg(short, long)]
    pub file: Option<String>,
    /// Do not use cache when building the image
    #[arg(long)]
    pub no_cache: bool,
}

#[derive(Debug, Parser)]
pub struct PsArgs {
    /// Show all containers (inluding stopped)
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Debug, Parser)]
pub struct BlueprintArgs {
    /// Show all blueprints (including imtermediate)
    #[arg(short, long)]
    pub all: String,
}

#[derive(Debug, Parser)]
pub struct StopArgs {
    /// Container name or IDs
    pub containers: Vec<String>,
    /// Force stop running containers
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Parser)]
pub struct RmArgs {
    /// Container names or IDs
    pub containers: Vec<String>,
    /// Force remove running containers
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Parser)]
    /// Blueprint names or IDs
pub struct RmbArgs {
    /// Blueprint names or IDs
    pub blueprints: Vec<String>,
    /// Force removal
    pub force: bool,
}

#[derive(Debug, Parser)]
pub struct PullArgs {
    /// Image ref (alpine:latest, ubuntu:22.04)
    pub image: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_root_dir_from_env_in_cli() {
        std::env::set_var("CUBO_ROOT", "/var/lib/cubo-test");
        // Parse a simple command without providing --root-dir
        let cli: Cli = Cli::parse_from(["cubo", "ps"]);
        assert_eq!(cli.root_dir, Some("/var/lib/cubo-test".to_string()));
    }

    #[test]
    #[serial]
    fn test_root_dir_flag_overrides_env() {
        std::env::set_var("CUBO_ROOT", "/env/path");
        let cli: Cli = Cli::parse_from(["cubo", "--root-dir", "/flag/path", "ps"]);
        assert_eq!(cli.root_dir, Some("/flag/path".to_string()));
    }
}