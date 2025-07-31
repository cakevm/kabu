use clap::{Parser, Subcommand};

#[derive(Debug, Subcommand)]
pub enum Command {
    Node(KabuArgs),
    Remote(KabuArgs),
}

#[derive(Parser, Debug)]
#[command(name="Kabu", version, about, long_about = None)]
pub struct AppArgs {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Parser, Debug)]
pub struct KabuArgs {
    #[arg(long, default_value = "config.toml")]
    pub kabu_config: String,
}
