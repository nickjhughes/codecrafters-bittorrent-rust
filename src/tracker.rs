use anyhow::{Context, Result};
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::{bencode::BencodeValue, torrent::Torrent, PEER_ID};

const PORT: u16 = 6881;

#[derive(Debug, Serialize)]
struct Request {
    peer_id: String,
    port: u16,
    uploaded: usize,
    downloaded: usize,
    left: usize,
    compact: u8,
}

impl Request {
    pub fn new(size: usize) -> Self {
        Request {
            peer_id: PEER_ID.to_owned(),
            port: PORT,
            uploaded: 0,
            downloaded: 0,
            left: size,
            compact: 1,
        }
    }
}

fn parse_peers(input: &[u8]) -> Result<Vec<SocketAddrV4>> {
    if input.len() % 6 != 0 {
        anyhow::bail!("invalid peers list");
    }

    let mut peers = Vec::new();
    for i in (0..input.len()).step_by(6) {
        let ip_addr = Ipv4Addr::new(input[i], input[i + 1], input[i + 2], input[i + 3]);
        let port = u16::from_be_bytes([input[i + 4], input[i + 5]]);
        peers.push(SocketAddrV4::new(ip_addr, port));
    }
    Ok(peers)
}

pub fn get_peers(torrent: &Torrent) -> Result<Vec<SocketAddrV4>> {
    let request_params = Request::new(torrent.info.length);
    let info_hash = torrent.info_hash();
    let mut url_encoded_info_hash = String::new();
    for i in 0..20 {
        url_encoded_info_hash.push('%');
        url_encoded_info_hash.push(info_hash.chars().nth(2 * i).unwrap());
        url_encoded_info_hash.push(info_hash.chars().nth(2 * i + 1).unwrap());
    }

    let client = reqwest::blocking::Client::new();
    let url = format!(
        "{}?info_hash={}&{}",
        torrent.announce,
        url_encoded_info_hash,
        serde_urlencoded::to_string(&request_params)?
    );
    let request = client.get(url);

    let response = request.send()?;
    if !response.status().is_success() {
        anyhow::bail!("peer request failed: {:?}", response.text());
    }
    let response_body = response.bytes()?;
    let (_, response_data) = BencodeValue::from_bytes(&response_body)?;
    for (key, value) in response_data.as_dictionary().context("invalid response")? {
        if std::str::from_utf8(key.0) == Ok("peers") {
            return parse_peers(value.as_byte_string().context("invalid response")?.0);
        }
    }
    Err(anyhow::format_err!(
        "no peers field found in response: {}",
        response_data
    ))
}
