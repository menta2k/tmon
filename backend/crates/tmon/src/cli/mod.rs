//! Command line interface.

pub mod geoip_update;
pub mod hash_password;
pub mod replay;
pub mod serve;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "tmon",
    version,
    about = "MikroTik NetFlow/IPFIX/sFlow traffic monitor"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run collectors, storage and the web UI/API.
    Serve {
        #[arg(long, env = "TMON_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Read a password from stdin and print its argon2id hash.
    HashPassword,
    /// Download the DB-IP Lite country/ASN databases into the `[geoip]` paths.
    GeoipUpdate {
        #[arg(long, env = "TMON_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Replay UDP datagrams from a pcap file to a collector.
    Replay {
        file: PathBuf,
        #[arg(long, default_value = "127.0.0.1:2055")]
        to: String,
        /// `1x` (original timing), `Nx`, or `max`.
        #[arg(long, default_value = "1x")]
        speed: String,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve { config } => serve::run(config.as_deref()).await,
        Command::HashPassword => hash_password::run(),
        Command::GeoipUpdate { config } => geoip_update::run(config.as_deref()).await,
        Command::Replay { file, to, speed } => replay::run(&file, &to, &speed).await,
    }
}
