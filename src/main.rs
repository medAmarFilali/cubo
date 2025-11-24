
use clap::Parser;

use cubo::cli::{self, Cli};
use cubo::commands;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging
    tracing_subscriber::fmt::init();

    let cli: Cli = Cli::parse();

    if let Some(ref root) = cli.root_dir {
        std::env::set_var("CUBO_ROOT", root);
    }

    println!("Cubo containerization tool");
    
    match cli.command {
        cli::Commands::Run(args) => commands::run::execute(args).await?,
        cli::Commands::Build(args) => commands::build::execute(args).await?,
        cli::Commands::Ps(args) => commands::ps::execute(args).await?,
        cli::Commands::Blueprint(args) => commands::blueprints::execute(args).await?,
        cli::Commands::Stop(args) => commands::stop::execute(args).await?,
        cli::Commands::Rm(args) => commands::rm::execute(args).await?,
        cli::Commands::Pull(args) => commands::pull::execute(args).await?
    }

    Ok(())
}