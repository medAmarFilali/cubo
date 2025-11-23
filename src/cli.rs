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
    #[arg(short, long)]
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

        let cli: Cli = Cli::parse_from(["cubo", "ps"]);
        assert_eq!(cli.root_dir, Some("/var/lib/cubo-test".to_string()));
        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    #[serial]
    fn test_root_dir_flag_overrides_env() {
        std::env::set_var("CUBO_ROOT", "/env/path");
        let cli: Cli = Cli::parse_from(["cubo", "--root-dir", "/flag/path", "ps"]);
        assert_eq!(cli.root_dir, Some("/flag/path".to_string()));
        std::env::remove_var("CUBO_ROOT");
    }

    #[test]
    #[serial]
    fn test_root_dir_not_set() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "ps"]);
        assert_eq!(cli.root_dir, None);
    }

    // Run command tests
    #[test]
    #[serial]
    fn test_run_command_basic() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "run", "alpine"]);
        if let Commands::Run(args) = cli.command {
            assert_eq!(args.blueprint, "alpine");
            assert!(args.command.is_none());
            assert!(args.name.is_none());
            assert!(!args.detach);
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    #[serial]
    fn test_run_command_with_all_options() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from([
            "cubo", "run", "ubuntu:22.04",
            "--name", "cubo-container",
            "-d",
            "-v", "/host:/container",
            "-v", "/tmp:/tmp:ro",
            "-p", "8080:80",
            "-e", "FOO=bar",
            "-e", "BAZ=baz",
            "-w", "/app",
            "--", "bash", "-c", "echo hello"
        ]);
        if let Commands::Run(args) = cli.command {
            assert_eq!(args.blueprint, "ubuntu:22.04");
            assert_eq!(args.name, Some("cubo-container".to_string()));
            assert!(args.detach);
            assert_eq!(args.volume.len(), 2);
            assert_eq!(args.volume[0], "/host:/container");
            assert_eq!(args.volume[1], "/tmp:/tmp:ro");
            assert_eq!(args.publish.len(), 1);
            assert_eq!(args.publish[0], "8080:80");
            assert_eq!(args.env.len(), 2);
            assert_eq!(args.env.len(), 2);
            assert_eq!(args.workdir, Some("/app".to_string()));
            let cmd = args.command.unwrap();
            assert_eq!(cmd, vec!["bash", "-c", "echo hello"])
        } else {
            panic!("Excpected Run command");
        }
    }

    // Build command tests
    #[test]
    #[serial]
    fn test_build_command_basic() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "build", "."]);
        if let Commands::Build(args) = cli.command {
            assert_eq!(args.path, ".");
            assert!(args.tag.is_none());
            assert!(args.file.is_none());
            assert!(!args.no_cache);
        } else {
            panic!("Excpected Run command");
        }
    }

    #[test]
    #[serial]
    fn test_build_command_with_options() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from([
            "cubo", "build", "/path/to/context",
            "theimage:v1.0",
            "-f", "Cubofile.custom",
            "--no-cache"
        ]);

        if let Commands::Build(args) = cli.command {
            assert_eq!(args.path, "/path/to/context");
            assert_eq!(args.tag, Some("theimage:v1.0".to_string()));
            assert_eq!(args.file, Some("Cubofile.custom".to_string()));
            assert!(args.no_cache);
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    #[serial]
    fn test_ps_command_basic() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "ps"]);
        if let Commands::Ps(args) = cli.command {
            assert!(!args.all);
        } else {
            panic!("Expected Ps command");
        }
    }

    #[test]
    #[serial]
    fn test_ps_command_with_all() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "ps", "-a"]);
        if let Commands::Ps(args) = cli.command {
            assert!(args.all)
        } else {
            panic!("Expected Ps command");
        }
    }

    #[test]
    #[serial]
    fn test_stop_command_single() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "stop", "container1"]);

        if let Commands::Stop(args) = cli.command {
            assert_eq!(args.containers, vec!["container1"]);
        } else {
            panic!("Expected stop command");
        }
    }

    #[test]
    #[serial]
    fn test_stop_command_multiple_with_force() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "stop", "-f", "c1", "c2", "c3"]);
        if let Commands::Stop(args) = cli.command {
            assert_eq!(args.containers, vec!["c1", "c2", "c3"]);
            assert!(args.force);
        } else {
            panic!("Expected Stop command");
        }
    }

    #[test]
    #[serial]
    fn test_rm_command_single() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "rm", "container1"]);
        if let Commands::Rm(args) = cli.command {
            assert_eq!(args.containers, vec!["container1"]);
            assert!(!args.force);
        } else {
            panic!("Expected Rm command");
        }
    }

    #[test]
    #[serial]
    fn test_rm_command_with_force() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "rm", "--force", "c1", "c2"]);
        if let Commands::Rm(args) = cli.command {
            assert_eq!(args.containers, vec!["c1", "c2"]);
            assert!(args.force);
        } else {
            panic!("Expected Rm command");
        }
    }

    #[test]
    #[serial]
    fn test_pull_command() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "pull", "alpine:latest"]);
        if let Commands::Pull(args) = cli.command {
            assert_eq!(args.image, "alpine:latest");
        } else {
            panic!("Expected Pull command");
        }
    }

    #[test]
    #[serial]
    fn test_pull_command_with_registry() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "pull", "ghcr.io/owner/image:tag"]);
        if let Commands::Pull(args) = cli.command {
            assert_eq!(args.image, "ghcr.io/owner/image:tag");
        } else {
            panic!("Expected Pull command");
        }
    }

    #[test]
    #[serial]
    fn test_blueprint_command() {
        std::env::remove_var("CUBO_ROOT");
        let cli = Cli::parse_from(["cubo", "blueprint", "-a", "true"]);
        if let Commands::Blueprint(args) = cli.command {
            assert_eq!(args.all, "true");
        } else {
            panic!("Expected Blueprint command");
        }
    }
}