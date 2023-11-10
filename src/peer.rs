use anyhow::Result;
use std::net::SocketAddrV4;
use tokio::{
    io::{self, AsyncWriteExt},
    net::TcpStream,
};

use crate::{torrent::Torrent, PEER_ID};

const HANDSHAKE_LEN: usize = 68;

struct Handshake {
    info_hash: [u8; 20],
    peer_id: [u8; 20],
}

impl Handshake {
    fn new(torrent: &Torrent) -> Result<Self> {
        Ok(Handshake {
            info_hash: hex::decode(torrent.info_hash())?.try_into().unwrap(),
            peer_id: PEER_ID.as_bytes().try_into()?,
        })
    }

    fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.push(19);
        output.extend(b"BitTorrent protocol");
        output.extend([0u8; 8]);
        output.extend(&self.info_hash);
        output.extend(&self.peer_id);

        assert_eq!(output.len(), HANDSHAKE_LEN);

        output
    }

    fn decode(input: &[u8]) -> Result<Self> {
        if input.len() < HANDSHAKE_LEN {
            anyhow::bail!("incomplete handshake");
        }
        if input[0] != 19 || &input[1..20] != b"BitTorrent protocol" {
            anyhow::bail!("unsupported protocol");
        }

        Ok(Handshake {
            info_hash: input[28..48].try_into()?,
            peer_id: input[48..68].try_into()?,
        })
    }
}

pub async fn handshake(torrent: &Torrent, peer_addr: SocketAddrV4) -> Result<[u8; 20]> {
    let mut stream = TcpStream::connect(peer_addr).await?;

    let request_handshake = Handshake::new(torrent)?;
    stream.write_all(&request_handshake.encode()).await?;

    let mut buf = [0; HANDSHAKE_LEN];
    let mut bytes_read = 0;
    loop {
        match stream.try_read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes_read += n;
                if bytes_read == HANDSHAKE_LEN {
                    break;
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    let response_handshake = Handshake::decode(&buf)?;
    Ok(response_handshake.peer_id)
}
