use anyhow::Result;
use clap::{Parser, Subcommand};
use std::{net::SocketAddrV4, path::PathBuf};

mod bencode;
mod peer;
mod torrent;
mod tracker;

const PEER_ID: &str = "27454831420650771739";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Decode {
        input: String,
    },
    Info {
        path: PathBuf,
    },
    Peers {
        path: PathBuf,
    },
    Handshake {
        path: PathBuf,
        peer_addr: SocketAddrV4,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Decode { input } => {
            let (_, value) = bencode::BencodeValue::from_str(&input)?;
            println!("{}", value);
        }
        Command::Info { path } => {
            let input = std::fs::read(path)?;
            let torrent = torrent::Torrent::from_bytes(&input)?;

            println!("Tracker URL: {}", torrent.announce);
            println!("Length: {}", torrent.info.length);
            println!("Info Hash: {}", torrent.info_hash());
            println!("Piece Length: {}", torrent.info.piece_length);
            println!("Piece Hashes:");
            for hash in torrent.info.piece_hashes().iter() {
                println!("{}", hash);
            }
        }
        Command::Peers { path } => {
            let input = std::fs::read(path)?;
            let torrent = torrent::Torrent::from_bytes(&input)?;

            for peer in tracker::get_peers(&torrent)?.iter() {
                println!("{:?}", peer);
            }
        }
        Command::Handshake { path, peer_addr } => {
            let input = std::fs::read(path)?;
            let torrent = torrent::Torrent::from_bytes(&input)?;

            let peer_id = peer::handshake(&torrent, peer_addr).await?;
            println!("Peer ID: {}", hex::encode(peer_id));
        }
    }

    Ok(())
}
