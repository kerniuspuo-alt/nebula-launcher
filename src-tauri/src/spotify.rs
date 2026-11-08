use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

const REDIRECT_URI: &str = "http://127.0.0.1:17385/callback";
const AUTH_ENDPOINT: &str = "https://accounts.spotify.com/authorize";
const TOKEN_ENDPOINT: &str = "https://accounts.spotify.com/api/token";
const SCOPES: &str = "streaming user-read-email user-read-private user-read-playback-state user-modify-playback-state playlist-read-private playlist-read-collaborative";

#[derive(Clone)]
struct PendingAuth {
    client_id: String,
    state: String,
    verifier: String,
}

static PENDING_AUTH: OnceLock<Mutex<Option<PendingAuth>>> = OnceLock::new();

fn pending_auth() -> &'static Mutex<Option<PendingAuth>> {
    PENDING_AUTH.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredToken {
    client_id: String,
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    scope: String,
}

#[derive(Clone, Serialize)]
struct AuthEvent {
    success: bool,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyProfile {
    id: String,
    display_name: String,
    email: Option<String>,
    product: Option<String>,
    image_url: Option<String>,
}

pub fn start(app: AppHandle) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 17385))?;
    std::thread::Builder::new()
        .name("nebula-spotify-callback".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                handle_callback(stream, &app);
            }
        })?;
    Ok(())
}

#[tauri::command]
pub fn spotify_authorize_url(client_id: String) -> Result<String, String> {
    let client_id = client_id.trim().to_string();
    if client_id.len() != 32 || !client_id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Enter a valid 32-character Spotify Client ID".into());
    }
    let state = uuid::Uuid::new_v4().simple().to_string();
    let verifier = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    *pending_auth().lock().map_err(|error| error.to_string())? = Some(PendingAuth {
        client_id: client_id.clone(),
        state: state.clone(),
        verifier,
    });
    let mut url = reqwest::Url::parse(AUTH_ENDPOINT).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &state)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", &challenge);
    Ok(url.to_string())
}

fn handle_callback(mut stream: TcpStream, app: &AppHandle) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let Some(path) = line.split_whitespace().nth(1) else {
        respond(&mut stream, "400 Bad Request", "Invalid Spotify callback.");
        return;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{path}")) else {
        respond(&mut stream, "400 Bad Request", "Invalid Spotify callback.");
        return;
    };
    if url.path() != "/callback" {
        respond(&mut stream, "404 Not Found", "Unknown callback path.");
        return;
    }
    let params = url.query_pairs().into_owned().collect::<std::collections::HashMap<_, _>>();
    let pending = match pending_auth().lock().ok().and_then(|mut auth| auth.take()) {
        Some(pending) => pending,
        None => {
            respond(&mut stream, "400 Bad Request", "No Spotify login is pending.");
            return;
        }
    };
    if params.get("state") != Some(&pending.state) {
        respond(&mut stream, "400 Bad Request", "Spotify login state did not match.");
        let _ = app.emit(
            "spotify-auth-status",
            AuthEvent { success: false, detail: "Spotify login state did not match".into() },
        );
        return;
    }
    if let Some(error) = params.get("error") {
        respond(&mut stream, "200 OK", "Spotify sign-in was cancelled. You can close this tab.");
        let _ = app.emit(
            "spotify-auth-status",
            AuthEvent { success: false, detail: format!("Spotify sign-in failed: {error}") },
        );
        return;
    }
    let Some(code) = params.get("code") else {
        respond(&mut stream, "400 Bad Request", "Spotify did not return an authorization code.");
        return;
    };
    match exchange_code(app, &pending, code) {
        Ok(()) => {
            respond(&mut stream, "200 OK", "Spotify is connected. You can close this tab.");
            let _ = app.emit(
                "spotify-auth-status",
                AuthEvent { success: true, detail: "Spotify connected".into() },
            );
        }
        Err(error) => {
            respond(&mut stream, "500 Internal Server Error", "Spotify sign-in failed. Return to Nebula for details.");
            let _ = app.emit("spotify-auth-status", AuthEvent { success: false, detail: error });
        }
    }
}

fn exchange_code(app: &AppHandle, pending: &PendingAuth, code: &str) -> Result<(), String> {
    let response: serde_json::Value = reqwest::blocking::Client::new()
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", pending.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", pending.verifier.as_str()),
        ])
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    save_token(
        app,
        StoredToken {
            client_id: pending.client_id.clone(),
            access_token: response["access_token"].as_str().ok_or("Spotify omitted access token")?.into(),
            refresh_token: response["refresh_token"].as_str().ok_or("Spotify omitted refresh token")?.into(),
            expires_at: chrono::Utc::now().timestamp() + response["expires_in"].as_i64().unwrap_or(3600),
            scope: response["scope"].as_str().unwrap_or(SCOPES).into(),
        },
    )
}

fn token_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("spotify_tokens.json"))
}

fn save_token(app: &AppHandle, token: StoredToken) -> Result<(), String> {
    let path = token_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, serde_json::to_vec(&token).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn spotify_access_token(app: AppHandle) -> Result<String, String> {
    access_token(&app).await
}

async fn access_token(app: &AppHandle) -> Result<String, String> {
    let path = token_path(app)?;
    let raw = std::fs::read(&path).map_err(|_| "Connect Spotify first".to_string())?;
    let mut token: StoredToken = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    if token.expires_at > chrono::Utc::now().timestamp() + 60 {
        return Ok(token.access_token);
    }
    let response: serde_json::Value = reqwest::Client::new()
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", token.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", token.refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    token.access_token = response["access_token"].as_str().ok_or("Spotify omitted refreshed access token")?.into();
    if let Some(refresh_token) = response["refresh_token"].as_str() {
        token.refresh_token = refresh_token.into();
    }
    token.expires_at = chrono::Utc::now().timestamp() + response["expires_in"].as_i64().unwrap_or(3600);
    save_token(app, token.clone())?;
    Ok(token.access_token)
}

#[tauri::command]
pub async fn spotify_current_user(app: AppHandle) -> Result<SpotifyProfile, String> {
    let token = access_token(&app).await?;
    let profile: serde_json::Value = reqwest::Client::new()
        .get("https://api.spotify.com/v1/me")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| format!("Spotify profile request failed: {error}"))?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    Ok(SpotifyProfile {
        id: profile.get("id").and_then(|value| value.as_str()).ok_or("Spotify profile omitted user ID")?.into(),
        display_name: profile.get("display_name").and_then(|value| value.as_str()).filter(|value| !value.is_empty()).or_else(|| profile.get("id").and_then(|value| value.as_str())).unwrap_or("Spotify user").into(),
        email: profile.get("email").and_then(|value| value.as_str()).map(str::to_string),
        product: profile.get("product").and_then(|value| value.as_str()).map(str::to_string),
        image_url: profile.pointer("/images/0/url").and_then(|value| value.as_str()).map(str::to_string),
    })
}

#[tauri::command]
pub fn spotify_disconnect(app: AppHandle) -> Result<(), String> {
    let path = token_path(&app)?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Nebula Spotify</title></head><body style=\"font:16px system-ui;background:#10131c;color:#eef2ff;padding:4rem\"><h1>{message}</h1></body></html>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

#[cfg(test)]
mod tests {
    use super::{spotify_authorize_url, REDIRECT_URI};

    #[test]
    fn creates_pkce_authorization_url() {
        let url = reqwest::Url::parse(&spotify_authorize_url("0123456789abcdefghijklmnopqrstuv".into()).unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("accounts.spotify.com"));
        assert_eq!(url.path(), "/authorize");
        assert_eq!(url.query_pairs().find(|(key, _)| key == "code_challenge_method").unwrap().1, "S256");
        assert_eq!(url.query_pairs().find(|(key, _)| key == "redirect_uri").unwrap().1, REDIRECT_URI);
    }

    #[test]
    fn rejects_invalid_spotify_client_ids() {
        assert!(spotify_authorize_url("short".into()).is_err());
        assert!(spotify_authorize_url("0123456789abcdefghijklmnopqrstuv!".into()).is_err());
    }
}