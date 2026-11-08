use serde::Serialize;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub host: String,
    pub port: u16,
    pub online: bool,
    pub ping_ms: Option<u128>,
    pub motd: Option<String>,
    pub players_online: Option<i64>,
    pub players_max: Option<i64>,
    pub version: Option<String>,
    pub favicon: Option<String>,
    pub error: Option<String>,
}

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        if (value & !0x7F) == 0 {
            buf.push(value as u8);
            return;
        }
        buf.push(((value & 0x7F) | 0x80) as u8);
        value = ((value as u32) >> 7) as i32;
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

fn read_varint_stream(stream: &mut TcpStream) -> std::io::Result<i32> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        let val = byte[0];
        result |= ((val & 0x7F) as i32) << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too long"));
        }
        if (val & 0x80) == 0 {
            break;
        }
    }
    Ok(result)
}

fn motd_from_json(v: &serde_json::Value) -> String {
    fn flatten(desc: &serde_json::Value, out: &mut String) {
        if let Some(s) = desc.as_str() {
            out.push_str(s);
            return;
        }
        if let Some(t) = desc.get("text").and_then(|t| t.as_str()) {
            out.push_str(t);
        }
        if let Some(extra) = desc.get("extra").and_then(|e| e.as_array()) {
            for e in extra {
                flatten(e, out);
            }
        }
    }
    let mut s = String::new();
    if let Some(desc) = v.get("description") {
        flatten(desc, &mut s);
    }
    s
}

fn fail(host: String, port: u16, ping_ms: Option<u128>, error: String) -> ServerStatus {
    ServerStatus {
        host,
        port,
        online: false,
        ping_ms,
        motd: None,
        players_online: None,
        players_max: None,
        version: None,
        favicon: None,
        error: Some(error),
    }
}

#[tauri::command]
pub fn ping_server(host: String, port: Option<u16>) -> ServerStatus {
    let port = port.unwrap_or(25565);
    let addr = format!("{host}:{port}");
    let start = Instant::now();
    let socket = match addr.to_socket_addrs() {
        Ok(mut iter) => match iter.next() {
            Some(s) => s,
            None => return fail(host, port, None, "DNS failed".into()),
        },
        Err(e) => return fail(host, port, None, e.to_string()),
    };
    let mut stream = match TcpStream::connect_timeout(&socket, Duration::from_secs(4)) {
        Ok(s) => s,
        Err(e) => return fail(host, port, None, e.to_string()),
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));

    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00);
    write_varint(&mut handshake, 760);
    write_string(&mut handshake, &host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);
    let mut packet = Vec::new();
    write_varint(&mut packet, handshake.len() as i32);
    packet.extend_from_slice(&handshake);
    packet.push(0x01);
    packet.push(0x00);
    if let Err(e) = stream.write_all(&packet) {
        return fail(host, port, None, e.to_string());
    }
    if read_varint_stream(&mut stream).is_err() {
        return fail(host, port, Some(start.elapsed().as_millis()), "bad length".into());
    }
    let _ = read_varint_stream(&mut stream);
    let json_len = match read_varint_stream(&mut stream) {
        Ok(n) => n as usize,
        Err(e) => return fail(host, port, Some(start.elapsed().as_millis()), e.to_string()),
    };
    let mut json_buf = vec![0u8; json_len.min(1_000_000)];
    if let Err(e) = stream.read_exact(&mut json_buf) {
        return fail(host, port, Some(start.elapsed().as_millis()), e.to_string());
    }
    let ping = start.elapsed().as_millis();
    let parsed: serde_json::Value = serde_json::from_slice(&json_buf).unwrap_or(serde_json::json!({}));
    ServerStatus {
        host,
        port,
        online: true,
        ping_ms: Some(ping),
        motd: Some(motd_from_json(&parsed)),
        players_online: parsed.pointer("/players/online").and_then(|v| v.as_i64()),
        players_max: parsed.pointer("/players/max").and_then(|v| v.as_i64()),
        version: parsed.pointer("/version/name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        favicon: parsed.get("favicon").and_then(|v| v.as_str()).map(str::to_string),
        error: None,
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CuratedServer {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub tags: Vec<String>,
    pub region: String,
}

#[tauri::command]
pub fn curated_servers() -> Vec<CuratedServer> {
    vec![
        CuratedServer { name: "Hypixel".into(), host: "mc.hypixel.net".into(), port: 25565, tags: vec!["Bedwars".into(), "Minigames".into(), "Skyblock".into()], region: "Global".into() },
        CuratedServer { name: "CubeCraft".into(), host: "play.cubecraft.net".into(), port: 25565, tags: vec!["Bedwars".into(), "Minigames".into(), "Skyblock".into()], region: "Global".into() },
        CuratedServer { name: "Wynncraft".into(), host: "play.wynncraft.com".into(), port: 25565, tags: vec!["MMORPG".into(), "Adventure".into()], region: "Global".into() },
        CuratedServer { name: "GommeHD".into(), host: "gommehd.net".into(), port: 25565, tags: vec!["Bedwars".into(), "Minigames".into(), "PvP".into()], region: "Europe".into() },
        CuratedServer { name: "Minemen Club".into(), host: "minemen.club".into(), port: 25565, tags: vec!["Practice".into(), "PvP".into()], region: "North America".into() },
        CuratedServer { name: "Minecraft Central".into(), host: "mccentral.org".into(), port: 25565, tags: vec!["Bedwars".into(), "Minigames".into(), "Survival".into()], region: "North America".into() },
        CuratedServer { name: "Purple Prison".into(), host: "purpleprison.net".into(), port: 25565, tags: vec!["Prison".into(), "PvP".into(), "Survival".into()], region: "North America".into() },
        CuratedServer { name: "2b2t".into(), host: "2b2t.org".into(), port: 25565, tags: vec!["Anarchy".into(), "Survival".into()], region: "Global".into() },
    ]
}
