use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod bencode;
mod torrent;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Decode { input: String },
    Info { path: PathBuf },
}

fn main() -> Result<()> {
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
                println!("{}", hex::encode(hash));
            }
        }
    }

    Ok(())
}
