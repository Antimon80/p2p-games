use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "p2p-games")]
#[command(about = "P2P Games: global chat & lobbies", long_about = None)]
struct Args {
    #[arg(short, long, default_value = "Player")]
    nickname: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Whoami,
    Say { text: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    tracing::info!("Starting CLI as {}", args.nickname);

    match args.command {
        Command::Whoami => {
            println!("(stub) node: not connected yet");
        }
        Command::Say { text } => {
            println!("(stub) would send to global: {}", text);
        }
    }

    Ok(())
}
