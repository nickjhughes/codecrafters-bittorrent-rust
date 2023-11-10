use anyhow::Result;
use sha1::{Digest, Sha1};
use std::{
    io::{Read, Write},
    net::SocketAddrV4,
    path::PathBuf,
};
use tempfile::TempDir;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{torrent::Torrent, PEER_ID};

const HANDSHAKE_LEN: usize = 68;
const BLOCK_LEN: usize = 16 * 1024;
const MAX_CONCURRENT_REQUESTS: usize = 5;

#[derive(Debug)]
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

enum PeerMessage {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
}

impl std::fmt::Debug for PeerMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerMessage::Choke => write!(f, "PeerMessage::Choke"),
            PeerMessage::Unchoke => write!(f, "PeerMessage::Unchoke"),
            PeerMessage::Interested => write!(f, "PeerMessage::Interested"),
            PeerMessage::NotInterested => write!(f, "PeerMessage::NotInterested"),
            PeerMessage::Have(index) => write!(f, "PeerMessage::Have({index})"),
            PeerMessage::Bitfield(bytes) => {
                write!(f, "PeerMessage::Bitfield(")?;
                for byte in bytes {
                    write!(f, "{:b}", byte)?;
                }
                write!(f, ")")
            }
            PeerMessage::Request {
                index,
                begin,
                length,
            } => write!(
                f,
                "PeerMessage::Request {{ index: {index}, begin: {begin}, length: {length} }}"
            ),
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => write!(
                f,
                "PeerMessage::Piece {{ index: {index}, begin: {begin}, block.len(): {} }}",
                block.len()
            ),
            PeerMessage::Cancel {
                index,
                begin,
                length,
            } => write!(
                f,
                "PeerMessage::Cancel {{ index: {index}, begin: {begin}, length: {length} }}"
            ),
        }
    }
}

impl PeerMessage {
    fn decode(input: &[u8]) -> Result<Self> {
        let tag = input[0];
        let payload = &input[1..];

        match tag {
            0 => Ok(PeerMessage::Choke),
            1 => Ok(PeerMessage::Unchoke),
            2 => Ok(PeerMessage::Interested),
            3 => Ok(PeerMessage::NotInterested),
            4 => {
                // Have
                Ok(PeerMessage::Have(u32::from_be_bytes([
                    payload[0], payload[1], payload[2], payload[3],
                ])))
            }
            5 => {
                // Bitfield
                Ok(PeerMessage::Bitfield(payload.to_vec()))
            }
            6 => {
                // Request
                Ok(PeerMessage::Request {
                    index: u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]),
                    begin: u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]),
                    length: u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]),
                })
            }
            7 => {
                // Piece
                Ok(PeerMessage::Piece {
                    index: u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]),
                    begin: u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]),
                    block: payload[8..].to_vec(),
                })
            }
            8 => {
                // Cancel
                Ok(PeerMessage::Cancel {
                    index: u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]),
                    begin: u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]),
                    length: u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]),
                })
            }
            _ => Err(anyhow::format_err!("invalid peer message tag {:?}", tag)),
        }
    }

    fn tag(&self) -> u8 {
        match self {
            PeerMessage::Choke => 0,
            PeerMessage::Unchoke => 1,
            PeerMessage::Interested => 2,
            PeerMessage::NotInterested => 3,
            PeerMessage::Have(_) => 4,
            PeerMessage::Bitfield(_) => 5,
            PeerMessage::Request { .. } => 6,
            PeerMessage::Piece { .. } => 7,
            PeerMessage::Cancel { .. } => 8,
        }
    }

    fn encode(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        match self {
            PeerMessage::Choke
            | PeerMessage::Unchoke
            | PeerMessage::Interested
            | PeerMessage::NotInterested => {
                output.extend(1u32.to_be_bytes());
                output.push(self.tag());
            }
            PeerMessage::Have(index) => {
                output.extend(5u32.to_be_bytes());
                output.push(self.tag());
                output.extend(index.to_be_bytes());
            }
            PeerMessage::Bitfield(bytes) => {
                output.extend((bytes.len() as u32 + 1).to_be_bytes());
                output.push(self.tag());
                output.extend(bytes);
            }
            PeerMessage::Request {
                index,
                begin,
                length,
            }
            | PeerMessage::Cancel {
                index,
                begin,
                length,
            } => {
                output.extend(13u32.to_be_bytes());
                output.push(self.tag());
                output.extend(index.to_be_bytes());
                output.extend(begin.to_be_bytes());
                output.extend(length.to_be_bytes());
            }
            PeerMessage::Piece {
                index,
                begin,
                block,
            } => {
                output.extend(((9 + block.len()) as u32).to_be_bytes());
                output.push(self.tag());
                output.extend(index.to_be_bytes());
                output.extend(begin.to_be_bytes());
                output.extend(block);
            }
        }
        Ok(output)
    }
}

#[derive(Debug, PartialEq)]
pub enum PeerConnectionState {
    Connected,
    WaitingForHandshake,
    WaitingForBitfield,
    ReadyToExpressInterest,
    WaitingForUnchoke,
    ReadyToRequest,
    GettingPieces,
}

pub struct PeerConnection {
    torrent: Torrent,
    state: PeerConnectionState,
    stream: TcpStream,
    pub peer_id: Option<[u8; 20]>,
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
enum BlockState {
    #[default]
    None,
    Requested,
    Downloaded,
}

impl PeerConnection {
    /// Connect and handshake with the given peer.
    pub async fn connect(torrent: Torrent, peer_addr: SocketAddrV4) -> Result<Self> {
        let stream = TcpStream::connect(peer_addr).await?;
        let mut connection = PeerConnection {
            torrent,
            state: PeerConnectionState::Connected,
            stream,
            peer_id: None,
        };

        connection.send_handshake().await?;
        connection.receive_handshake().await?;

        Ok(connection)
    }

    async fn send_handshake(&mut self) -> Result<()> {
        let handshake_request = Handshake::new(&self.torrent)?;
        self.stream.write_all(&handshake_request.encode()).await?;
        self.state = PeerConnectionState::WaitingForHandshake;
        Ok(())
    }

    async fn receive_handshake(&mut self) -> Result<()> {
        assert_eq!(self.state, PeerConnectionState::WaitingForHandshake);
        let mut buf = [0; HANDSHAKE_LEN];
        self.stream.read_exact(&mut buf).await?;
        let handshake_response = Handshake::decode(&buf)?;
        self.peer_id = Some(handshake_response.peer_id);
        self.state = PeerConnectionState::WaitingForBitfield;
        Ok(())
    }

    async fn send_message(&mut self, msg: PeerMessage) -> Result<()> {
        self.stream.write_all(&msg.encode()?).await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<PeerMessage> {
        let mut length_buf = [0; 4];
        match self.stream.read_exact(&mut length_buf).await {
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                anyhow::bail!("connected reset by peer")
            }
            Err(e) => anyhow::bail!("failed to read from stream: {:?}", e),
            _ => {}
        };
        let length =
            u32::from_be_bytes([length_buf[0], length_buf[1], length_buf[2], length_buf[3]])
                as usize;

        let mut msg_buf = vec![0; length];
        self.stream.read_exact(&mut msg_buf).await?;
        let msg = PeerMessage::decode(&msg_buf)?;
        Ok(msg)
    }

    async fn receive_bitfield(&mut self) -> Result<()> {
        assert_eq!(self.state, PeerConnectionState::WaitingForBitfield);
        let message = self.receive_message().await?;
        match message {
            PeerMessage::Bitfield(_) => {
                // Ignore bitfields for this challenge
                self.state = PeerConnectionState::ReadyToExpressInterest;
            }
            _ => anyhow::bail!("unexpected message {:?}", message),
        }
        Ok(())
    }

    pub async fn download<P>(&mut self, output_path: P) -> Result<()>
    where
        P: Into<PathBuf>,
    {
        let temp_dir = TempDir::new()?;
        for i in 0..self.torrent.info.piece_count() {
            let piece_path = {
                let mut p = PathBuf::from(temp_dir.path());
                p.push(format!("piece-{}", i));
                p
            };
            self.download_piece(i, &piece_path).await?;
        }

        let mut file = std::fs::File::create(output_path.into())?;
        let mut piece_buf = Vec::with_capacity(self.torrent.info.piece_length);
        for i in 0..self.torrent.info.piece_count() {
            piece_buf.clear();
            let piece_path = {
                let mut p = PathBuf::from(temp_dir.path());
                p.push(format!("piece-{}", i));
                p
            };
            let mut piece_file = std::fs::File::open(piece_path)?;
            piece_file.read_to_end(&mut piece_buf)?;
            file.write_all(&piece_buf)?;
        }

        Ok(())
    }

    pub async fn download_piece<P>(&mut self, piece_index: usize, output_path: P) -> Result<()>
    where
        P: Into<PathBuf>,
    {
        match self.state {
            PeerConnectionState::WaitingForBitfield => {
                self.receive_bitfield().await?;
            }
            PeerConnectionState::ReadyToExpressInterest | PeerConnectionState::ReadyToRequest => {}
            _ => anyhow::bail!("invalid state {:?}", self.state),
        }

        let piece_length = if piece_index == self.torrent.info.piece_count() - 1 {
            if self.torrent.info.length % self.torrent.info.piece_length == 0 {
                self.torrent.info.piece_length
            } else {
                self.torrent.info.length % self.torrent.info.piece_length
            }
        } else {
            self.torrent.info.piece_length
        };
        let block_count = div_round_up(piece_length, BLOCK_LEN);
        let mut block_states = vec![BlockState::default(); block_count];
        let last_block_len = if piece_length % BLOCK_LEN == 0 {
            BLOCK_LEN
        } else {
            piece_length % BLOCK_LEN
        };
        let mut piece = vec![0; piece_length];

        loop {
            match self.state {
                PeerConnectionState::ReadyToExpressInterest => {
                    self.send_message(PeerMessage::Interested).await?;
                    self.state = PeerConnectionState::WaitingForUnchoke;
                }
                PeerConnectionState::WaitingForUnchoke => {
                    let msg = self.receive_message().await?;
                    if let PeerMessage::Unchoke = msg {
                        self.state = PeerConnectionState::ReadyToRequest;
                    }
                }
                PeerConnectionState::ReadyToRequest => {
                    for (i, block_state) in block_states
                        .iter_mut()
                        .enumerate()
                        .take(block_count.min(MAX_CONCURRENT_REQUESTS))
                    {
                        let msg = PeerMessage::Request {
                            index: piece_index as u32,
                            begin: (i * BLOCK_LEN) as u32,
                            length: if i == block_count - 1 {
                                last_block_len
                            } else {
                                BLOCK_LEN
                            } as u32,
                        };
                        self.send_message(msg).await?;
                        *block_state = BlockState::Requested;
                    }
                    self.state = PeerConnectionState::GettingPieces;
                }
                PeerConnectionState::GettingPieces => {
                    let msg = self.receive_message().await?;
                    if let PeerMessage::Piece {
                        index,
                        begin,
                        block,
                    } = msg
                    {
                        if index as usize != piece_index {
                            eprintln!("received block of a different piece");
                            continue;
                        }
                        let block_index = begin as usize / BLOCK_LEN;
                        block_states[block_index] = BlockState::Downloaded;
                        let block_len = if block_index == block_count - 1 {
                            last_block_len
                        } else {
                            BLOCK_LEN
                        };
                        piece[begin as usize..begin as usize + block_len].copy_from_slice(&block);

                        let next_block_index =
                            block_states.iter().position(|s| *s == BlockState::None);
                        if let Some(next_block_index) = next_block_index {
                            let msg = PeerMessage::Request {
                                index: piece_index as u32,
                                begin: (next_block_index * BLOCK_LEN) as u32,
                                length: if next_block_index == block_count - 1 {
                                    last_block_len
                                } else {
                                    BLOCK_LEN
                                } as u32,
                            };
                            self.send_message(msg).await?;
                            block_states[next_block_index] = BlockState::Requested;
                        } else if block_states.iter().all(|s| *s == BlockState::Downloaded) {
                            // All blocks downloaded
                            self.state = PeerConnectionState::ReadyToRequest;
                            break;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        let piece_hash = {
            let mut hasher = Sha1::new();
            hasher.update(&piece);
            let result = hasher.finalize();
            hex::encode(result)
        };
        if piece_hash != *self.torrent.info.piece_hashes().get(piece_index).unwrap() {
            anyhow::bail!("incorrect piece hash");
        }

        std::fs::write(output_path.into(), &piece)?;
        Ok(())
    }
}

pub fn div_round_up(a: usize, b: usize) -> usize {
    (a + (b - 1)) / b
}
