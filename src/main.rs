use anyhow::Result;
use clap::{Parser, Subcommand};

mod bencode;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Decode { input: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Decode { input } => {
            println!("{:?}", bencode::BencodeValue::from_str(&input)?);
        }
    }

    Ok(())
}
