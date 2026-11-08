use serde::Deserialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use tauri::Emitter;

#[derive(Deserialize)]
struct FpsSample {
    fps: u32,
}

pub fn start(app: tauri::AppHandle) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 17384))?;
    thread::Builder::new()
        .name("nebula-overlay-telemetry".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                handle_sample(stream, &app);
            }
        })?;
    Ok(())
}

fn handle_sample(mut stream: TcpStream, app: &tauri::AppHandle) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    if request_line.trim_end() != "POST /fps HTTP/1.1" {
        respond(&mut stream, "404 Not Found");
        return;
    }

    let mut content_length = None;
    let mut valid_host = false;
    let mut valid_content_type = false;
    let mut has_origin = false;
    let mut header_bytes = request_line.len();
    loop {
        let mut line = String::new();
        let Ok(bytes) = reader.read_line(&mut line) else {
            respond(&mut stream, "400 Bad Request");
            return;
        };
        header_bytes += bytes;
        if header_bytes > 4096 {
            respond(&mut stream, "400 Bad Request");
            return;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse::<usize>().ok();
            } else if name.eq_ignore_ascii_case("host") {
                valid_host = value.eq_ignore_ascii_case("127.0.0.1:17384");
            } else if name.eq_ignore_ascii_case("content-type") {
                valid_content_type = value
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"));
            } else if name.eq_ignore_ascii_case("origin") {
                has_origin = true;
            }
        }
    }

    if !valid_host || !valid_content_type || has_origin {
        respond(&mut stream, "403 Forbidden");
        return;
    }

    let Some(length) = content_length.filter(|length| *length <= 128) else {
        respond(&mut stream, "400 Bad Request");
        return;
    };
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        respond(&mut stream, "400 Bad Request");
        return;
    }
    match serde_json::from_slice::<FpsSample>(&body) {
        Ok(sample) if sample.fps <= 10_000 => {
            let _ = app.emit("overlay-fps", sample.fps);
            respond(&mut stream, "204 No Content");
        }
        _ => respond(&mut stream, "400 Bad Request"),
    }
}

fn respond(stream: &mut TcpStream, status: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}