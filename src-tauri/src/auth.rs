use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::paths;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftProfile {
    pub uuid: String,
    pub name: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: String,
    pub expires_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
struct AccountFile {
    accounts: Vec<MinecraftProfile>,
    active_uuid: Option<String>,
}

fn accounts_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(paths::app_dir(app)?.join("accounts.json"))
}

fn load(app: &AppHandle) -> Result<AccountFile, String> {
    let path = accounts_path(app)?;
    if !path.exists() {
        return Ok(AccountFile::default());
    }
    paths::read_json(&path)
}

fn save(app: &AppHandle, data: &AccountFile) -> Result<(), String> {
    paths::write_json(&accounts_path(app)?, data)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeStart {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
    pub message: String,
    pub client_id: String,
}

fn client_id() -> Result<String, String> {
    std::env::var("NEBULA_MS_CLIENT_ID")
        .or_else(|_| std::env::var("MICROSOFT_CLIENT_ID"))
        .map_err(|_| {
            "Set NEBULA_MS_CLIENT_ID to your Azure public client id (Xbox Live / Minecraft API). See README.".into()
        })
}

#[tauri::command]
pub async fn ms_device_code_start() -> Result<DeviceCodeStart, String> {
    let client_id = client_id()?;
    let http = reqwest::Client::new();
    let res = http
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    if json.get("error").is_some() {
        return Err(json.to_string());
    }
    Ok(DeviceCodeStart {
        user_code: json["user_code"].as_str().unwrap_or("").into(),
        device_code: json["device_code"].as_str().unwrap_or("").into(),
        verification_uri: json["verification_uri"]
            .as_str()
            .unwrap_or("https://www.microsoft.com/link")
            .into(),
        interval: json["interval"].as_u64().unwrap_or(5),
        expires_in: json["expires_in"].as_u64().unwrap_or(900),
        message: json["message"].as_str().unwrap_or("").into(),
        client_id,
    })
}

#[tauri::command]
pub async fn ms_device_code_poll(app: AppHandle, device_code: String, client_id: String) -> Result<MinecraftProfile, String> {
    let http = reqwest::Client::new();
    let token_res = http
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let token_json: serde_json::Value = token_res.json().await.map_err(|e| e.to_string())?;
    if let Some(err) = token_json.get("error").and_then(|e| e.as_str()) {
        return Err(err.into());
    }
    let ms_access = token_json["access_token"].as_str().ok_or("no access_token")?.to_string();
    let refresh = token_json["refresh_token"].as_str().map(|s| s.to_string());

    let xbox = xbox_live_auth(&http, &ms_access).await?;
    let xsts = xsts_auth(&http, &xbox).await?;
    let uhs = xsts
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|v| v.as_str())
        .ok_or("missing uhs")?;
    let xsts_token = xsts["Token"].as_str().ok_or("missing XSTS token")?;
    let mc = minecraft_login(&http, uhs, xsts_token).await?;
    let profile = minecraft_profile(&http, &mc).await?;

    let account = MinecraftProfile {
        uuid: profile["id"].as_str().unwrap_or("").into(),
        name: profile["name"].as_str().unwrap_or("").into(),
        access_token: mc,
        refresh_token: refresh,
        client_id,
        expires_at: None,
    };
    if account.uuid.is_empty() {
        return Err("This Microsoft account does not own Minecraft Java Edition.".into());
    }
    let mut data = load(&app)?;
    data.accounts.retain(|a| a.uuid != account.uuid);
    data.accounts.push(account.clone());
    data.active_uuid = Some(account.uuid.clone());
    save(&app, &data)?;
    let mut safe_account = account;
    safe_account.access_token = "<redacted>".into();
    safe_account.refresh_token = None;
    Ok(safe_account)
}

async fn xbox_live_auth(http: &reqwest::Client, ms_access: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    let res: serde_json::Value = http
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    res["Token"].as_str().map(|s| s.to_string()).ok_or_else(|| format!("Xbox Live auth failed: {res}"))
}

async fn xsts_auth(http: &reqwest::Client, xbox_token: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbox_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    http.post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

async fn minecraft_login(http: &reqwest::Client, uhs: &str, xsts: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{xsts}")
    });
    let res: serde_json::Value = http
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    res["access_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Minecraft services login failed — confirm the account owns Java Edition.".into())
}

async fn minecraft_profile(http: &reqwest::Client, mc_token: &str) -> Result<serde_json::Value, String> {
    http.get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_accounts(app: AppHandle) -> Result<Vec<MinecraftProfile>, String> {
    let mut accounts = load(&app)?.accounts;
    for a in &mut accounts {
        a.access_token = "<redacted>".into();
        a.refresh_token = None;
    }
    Ok(accounts)
}

#[tauri::command]
pub fn active_account(app: AppHandle) -> Result<Option<MinecraftProfile>, String> {
    let data = load(&app)?;
    Ok(data.accounts.into_iter().find(|a| Some(&a.uuid) == data.active_uuid.as_ref()).map(|mut a| {
        a.access_token = "<redacted>".into();
        a.refresh_token = None;
        a
    }))
}

#[tauri::command]
pub fn set_active_account(app: AppHandle, uuid: String) -> Result<(), String> {
    let mut data = load(&app)?;
    data.active_uuid = Some(uuid);
    save(&app, &data)
}

#[tauri::command]
pub fn remove_account(app: AppHandle, uuid: String) -> Result<(), String> {
    let mut data = load(&app)?;
    data.accounts.retain(|a| a.uuid != uuid);
    if data.active_uuid.as_deref() == Some(&uuid) {
        data.active_uuid = data.accounts.first().map(|a| a.uuid.clone());
    }
    save(&app, &data)
}

pub fn active_token(app: &AppHandle) -> Result<MinecraftProfile, String> {
    let data = load(app)?;
    data.accounts
        .into_iter()
        .find(|a| Some(&a.uuid) == data.active_uuid.as_ref())
        .ok_or_else(|| "Sign in with a Microsoft account that owns Minecraft Java Edition.".into())
}

#[derive(Serialize, Deserialize, Default)]
struct LockFile {
    pin_hash: Option<String>,
}

#[tauri::command]
pub fn set_pin(app: AppHandle, pin: String) -> Result<(), String> {
    if pin.len() != 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN must be 4 digits.".into());
    }
    let hash = hex::encode(Sha256::digest(format!("nebula-pin:{pin}").as_bytes()));
    paths::write_json(&paths::app_dir(&app)?.join("lock.json"), &LockFile { pin_hash: Some(hash) })
}

#[tauri::command]
pub fn clear_pin(app: AppHandle, pin: String) -> Result<(), String> {
    if pin.len() != 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("Current PIN must be 4 digits.".into());
    }

    let path = paths::app_dir(&app)?.join("lock.json");
    if !path.exists() {
        return Err("No PIN is configured.".into());
    }
    let lock: LockFile = paths::read_json(&path)?;
    let Some(expected) = lock.pin_hash else {
        return Err("No PIN is configured.".into());
    };
    let hash = hex::encode(Sha256::digest(format!("nebula-pin:{pin}").as_bytes()));
    if hash != expected {
        return Err("Current PIN is incorrect.".into());
    }
    std::fs::remove_file(path).map_err(|error| format!("Could not remove PIN lock: {error}"))
}

#[tauri::command]
pub fn change_pin(app: AppHandle, current_pin: String, new_pin: String) -> Result<(), String> {
    if current_pin.len() != 4 || !current_pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("Current PIN must be 4 digits.".into());
    }
    if new_pin.len() != 4 || !new_pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("New PIN must be 4 digits.".into());
    }

    let path = paths::app_dir(&app)?.join("lock.json");
    if !path.exists() {
        return Err("No PIN is configured.".into());
    }
    let lock: LockFile = paths::read_json(&path)?;
    let Some(expected) = lock.pin_hash else {
        return Err("No PIN is configured.".into());
    };
    let current_hash = hex::encode(Sha256::digest(format!("nebula-pin:{current_pin}").as_bytes()));
    if current_hash != expected {
        return Err("Current PIN is incorrect.".into());
    }

    let new_hash = hex::encode(Sha256::digest(format!("nebula-pin:{new_pin}").as_bytes()));
    paths::write_json(&path, &LockFile { pin_hash: Some(new_hash) })
}

#[tauri::command]
pub fn verify_pin(app: AppHandle, pin: String) -> Result<bool, String> {
    let path = paths::app_dir(&app)?.join("lock.json");
    if !path.exists() {
        return Ok(true);
    }
    let lock: LockFile = paths::read_json(&path)?;
    let Some(expected) = lock.pin_hash else {
        return Ok(true);
    };
    let hash = hex::encode(Sha256::digest(format!("nebula-pin:{pin}").as_bytes()));
    Ok(hash == expected)
}

#[tauri::command]
pub fn has_pin(app: AppHandle) -> Result<bool, String> {
    let path = paths::app_dir(&app)?.join("lock.json");
    if !path.exists() {
        return Ok(false);
    }
    let lock: LockFile = paths::read_json(&path)?;
    Ok(lock.pin_hash.is_some())
}
