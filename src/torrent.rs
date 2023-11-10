use anyhow::{Context, Result};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;

use crate::bencode::{BencodeByteString, BencodeValue};

#[derive(Debug)]
pub struct Torrent {
    pub announce: reqwest::Url,
    pub info: TorrentInfo,
}

#[derive(Debug)]
pub struct TorrentInfo {
    pub length: usize,
    pub name: String,
    pub piece_length: usize,
    pub pieces: Vec<u8>,
}

impl Torrent {
    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        let (_, value) = BencodeValue::from_bytes(input)?;
        let dict = value.as_dictionary().context("invalid torrent file")?;

        let announce = dict
            .get(&BencodeByteString(b"announce"))
            .and_then(BencodeValue::as_byte_string)
            .and_then(|bs| std::str::from_utf8(bs.0).ok())
            .and_then(|s| reqwest::Url::parse(s).ok())
            .context("missing or invalid announce field")?;

        let info = dict
            .get(&BencodeByteString(b"info"))
            .and_then(BencodeValue::as_dictionary)
            .context("missing or invalid info field")?;
        let length = info
            .get(&BencodeByteString(b"length"))
            .and_then(BencodeValue::as_integer)
            .and_then(|n| usize::try_from(*n).ok())
            .context("missing or invalid length field")?;
        let name = info
            .get(&BencodeByteString(b"name"))
            .and_then(BencodeValue::as_byte_string)
            .and_then(|bs| std::str::from_utf8(bs.0).ok())
            .context("missing or invalid name field")?
            .to_owned();
        let piece_length = info
            .get(&BencodeByteString(b"piece length"))
            .and_then(BencodeValue::as_integer)
            .and_then(|n| usize::try_from(*n).ok())
            .context("missing or invalid piece length field")?;
        let pieces = info
            .get(&BencodeByteString(b"pieces"))
            .and_then(BencodeValue::as_byte_string)
            .map(|bs| bs.0.to_vec())
            .context("missing or invalid pieces field")?;
        if pieces.len() % 20 != 0 {
            anyhow::bail!("invalid pieces field");
        }

        Ok(Torrent {
            announce,
            info: TorrentInfo {
                length,
                name,
                piece_length,
                pieces,
            },
        })
    }

    pub fn info_hash(&self) -> String {
        let info_bencode = BencodeValue::Dictionary(
            [
                (
                    BencodeByteString(b"length"),
                    BencodeValue::Integer(self.info.length as i64),
                ),
                (
                    BencodeByteString(b"name"),
                    BencodeValue::ByteString(BencodeByteString(self.info.name.as_bytes())),
                ),
                (
                    BencodeByteString(b"piece length"),
                    BencodeValue::Integer(self.info.piece_length as i64),
                ),
                (
                    BencodeByteString(b"pieces"),
                    BencodeValue::ByteString(BencodeByteString(&self.info.pieces)),
                ),
            ]
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
        );

        let mut hasher = Sha1::new();
        hasher.update(info_bencode.to_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }
}

impl TorrentInfo {
    pub fn piece_hashes(&self) -> Vec<String> {
        let mut output = Vec::new();
        for i in 0..self.pieces.len() / 20 {
            let piece_hash = &self.pieces[i * 20..(i + 1) * 20];
            output.push(hex::encode(piece_hash));
        }
        output
    }
}
