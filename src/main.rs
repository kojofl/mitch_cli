use clap::{Parser, Subcommand};
use daemon::Daemon;
use tokio::task::LocalSet;
mod client;
mod daemon;
pub mod mitch;
mod protocol;

#[derive(Debug, Parser)]
#[clap(name = "my-ble-tool", version = "0.1.0")]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    DaemonStart,

    Scan {
        #[clap(short, long, default_value_t = 2000)]
        timeout: u64,
    },

    Connect {
        name: String,
    },

    Disconnect {
        name: String,
    },

    Record {
        #[clap(short, long)]
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        Command::DaemonStart => {
            println!("Starting daemon...");
            let localset = LocalSet::new();
            localset.run_until(Daemon::new().await?.run()).await?;
        }
        Command::Scan { timeout } => {
            client::run_client(protocol::ClientCommand::Scan {
                timeout_ms: timeout,
            })
            .await?;
        }
        Command::Connect { name } => {
            client::run_client(protocol::ClientCommand::Connect { name }).await?;
        }
        Command::Disconnect { name } => {
            client::run_client(protocol::ClientCommand::Disconnect { name }).await?;
        }
        Command::Record { name } => {
            client::run_client(protocol::ClientCommand::Record { name }).await?
        }
    }

    Ok(())
}
