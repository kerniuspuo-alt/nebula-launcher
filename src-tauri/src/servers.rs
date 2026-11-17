use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpStream};

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

async fn read_varint_stream(stream: &mut TcpStream) -> io::Result<i32> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        let val = byte[0];
        if num_read == 5 || (num_read == 4 && val & 0xF0 != 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "varint too long",
            ));
        }
        result |= ((val & 0x7F) as i32) << (7 * num_read);
        num_read += 1;
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
pub async fn ping_server(host: String, port: Option<u16>) -> ServerStatus {
    let port = port.unwrap_or(25565);
    let key = (host.to_ascii_lowercase(), port);
    let cached_entry = match server_cache().lock() {
        Ok(cache) => cache.get(&key).cloned(),
        Err(error) => {
            eprintln!("Server status cache is unavailable: {error}");
            None
        }
    };
    if let Some((cached, cached_at)) = cached_entry {
        let ttl = if cached.online { 20 } else { 5 };
        if cached_at.elapsed() < Duration::from_secs(ttl) {
            return cached;
        }
    }

    let status = ping_server_uncached(host, port).await;
    if status
        .favicon
        .as_ref()
        .is_some_and(|favicon| favicon.len() > 64 * 1024)
    {
        return status;
    }
    match server_cache().lock() {
        Ok(mut cache) => {
            if cache.len() >= 128 {
                cache.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(30));
                if cache.len() >= 128 {
                    cache.clear();
                }
            }
            cache.insert(key, (status.clone(), Instant::now()));
        }
        Err(error) => eprintln!("Could not update server status cache: {error}"),
    }
    status
}

fn server_cache() -> &'static Mutex<HashMap<(String, u16), (ServerStatus, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, u16), (ServerStatus, Instant)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn ping_server_uncached(host: String, port: u16) -> ServerStatus {
    let start = Instant::now();
    let lookup_name = host.clone();
    let address = match tokio::time::timeout(
        Duration::from_secs(4),
        lookup_host((lookup_name.as_str(), port)),
    )
    .await
    {
        Ok(Ok(mut addresses)) => match addresses.next() {
            Some(address) => address,
            None => return fail(host, port, None, "DNS failed".into()),
        },
        Ok(Err(error)) => return fail(host, port, None, error.to_string()),
        Err(_) => return fail(host, port, None, "DNS lookup timed out".into()),
    };
    let mut stream =
        match tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(address)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return fail(host, port, None, e.to_string()),
            Err(_) => return fail(host, port, None, "Connection timed out".into()),
        };
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
    match tokio::time::timeout(Duration::from_secs(4), stream.write_all(&packet)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return fail(host, port, None, error.to_string()),
        Err(_) => return fail(host, port, None, "Connection write timed out".into()),
    }
    let frame_len =
        match tokio::time::timeout(Duration::from_secs(4), read_varint_stream(&mut stream)).await {
            Ok(Ok(n)) if n >= 0 => n as usize,
            Ok(Ok(_)) => {
                return fail(
                    host,
                    port,
                    Some(start.elapsed().as_millis()),
                    "bad length".into(),
                )
            }
            Ok(Err(e)) => {
                return fail(host, port, Some(start.elapsed().as_millis()), e.to_string())
            }
            Err(_) => {
                return fail(
                    host,
                    port,
                    Some(start.elapsed().as_millis()),
                    "Server response timed out".into(),
                )
            }
        };
    if frame_len > 1_000_000 {
        return fail(
            host,
            port,
            Some(start.elapsed().as_millis()),
            "server response too large".into(),
        );
    }
    if frame_len == 0 {
        return fail(
            host,
            port,
            Some(start.elapsed().as_millis()),
            "empty status packet".into(),
        );
    }
    let packet_id =
        match tokio::time::timeout(Duration::from_secs(4), read_varint_stream(&mut stream)).await {
            Ok(Ok(id)) => id,
            Ok(Err(e)) => {
                return fail(host, port, Some(start.elapsed().as_millis()), e.to_string())
            }
            Err(_) => {
                return fail(
                    host,
                    port,
                    Some(start.elapsed().as_millis()),
                    "Server response timed out".into(),
                )
            }
        };
    if packet_id != 0 {
        return fail(
            host,
            port,
            Some(start.elapsed().as_millis()),
            "unexpected status packet".into(),
        );
    }
    let json_len =
        match tokio::time::timeout(Duration::from_secs(4), read_varint_stream(&mut stream)).await {
            Ok(Ok(n)) if n >= 0 && n as usize <= frame_len => n as usize,
            Ok(Ok(_)) => {
                return fail(
                    host,
                    port,
                    Some(start.elapsed().as_millis()),
                    "bad response size".into(),
                )
            }
            Ok(Err(e)) => {
                return fail(host, port, Some(start.elapsed().as_millis()), e.to_string())
            }
            Err(_) => {
                return fail(
                    host,
                    port,
                    Some(start.elapsed().as_millis()),
                    "Server response timed out".into(),
                )
            }
        };
    let mut json_buf = vec![0u8; json_len];
    match tokio::time::timeout(Duration::from_secs(4), stream.read_exact(&mut json_buf)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            return fail(
                host,
                port,
                Some(start.elapsed().as_millis()),
                error.to_string(),
            )
        }
        Err(_) => {
            return fail(
                host,
                port,
                Some(start.elapsed().as_millis()),
                "Server response timed out".into(),
            )
        }
    }
    let ping = start.elapsed().as_millis();
    let parsed: serde_json::Value = match serde_json::from_slice(&json_buf) {
        Ok(parsed) => parsed,
        Err(error) => return fail(host, port, Some(ping), error.to_string()),
    };
    ServerStatus {
        host,
        port,
        online: true,
        ping_ms: Some(ping),
        motd: Some(motd_from_json(&parsed)),
        players_online: parsed.pointer("/players/online").and_then(|v| v.as_i64()),
        players_max: parsed.pointer("/players/max").and_then(|v| v.as_i64()),
        version: parsed
            .pointer("/version/name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        favicon: parsed
            .get("favicon")
            .and_then(|v| v.as_str())
            .map(str::to_string),
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
        CuratedServer {
            name: "Hypixel".into(),
            host: "mc.hypixel.net".into(),
            port: 25565,
            tags: vec!["Bedwars".into(), "Minigames".into(), "Skyblock".into()],
            region: "Global".into(),
        },
        CuratedServer {
            name: "CubeCraft".into(),
            host: "play.cubecraft.net".into(),
            port: 25565,
            tags: vec!["Bedwars".into(), "Minigames".into(), "Skyblock".into()],
            region: "Global".into(),
        },
        CuratedServer {
            name: "Wynncraft".into(),
            host: "play.wynncraft.com".into(),
            port: 25565,
            tags: vec!["MMORPG".into(), "Adventure".into()],
            region: "Global".into(),
        },
        CuratedServer {
            name: "GommeHD".into(),
            host: "gommehd.net".into(),
            port: 25565,
            tags: vec!["Bedwars".into(), "Minigames".into(), "PvP".into()],
            region: "Europe".into(),
        },
        CuratedServer {
            name: "Minemen Club".into(),
            host: "minemen.club".into(),
            port: 25565,
            tags: vec!["Practice".into(), "PvP".into()],
            region: "North America".into(),
        },
        CuratedServer {
            name: "Minecraft Central".into(),
            host: "mccentral.org".into(),
            port: 25565,
            tags: vec!["Bedwars".into(), "Minigames".into(), "Survival".into()],
            region: "North America".into(),
        },
        CuratedServer {
            name: "Purple Prison".into(),
            host: "purpleprison.net".into(),
            port: 25565,
            tags: vec!["Prison".into(), "PvP".into(), "Survival".into()],
            region: "North America".into(),
        },
        CuratedServer {
            name: "2b2t".into(),
            host: "2b2t.org".into(),
            port: 25565,
            tags: vec!["Anarchy".into(), "Survival".into()],
            region: "Global".into(),
        },
    ]
}
