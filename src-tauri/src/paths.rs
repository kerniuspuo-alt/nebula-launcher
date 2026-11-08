use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let data_dir = app_dir(app)?;
    for name in [
        "instances",
        "runtimes",
        "meta",
        "libraries",
        "assets",
        "versions",
    ] {
        std::fs::create_dir_all(data_dir.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn instances_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("instances");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn runtimes_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("runtimes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn meta_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("meta");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn libraries_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("libraries");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn assets_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("assets");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn versions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("versions");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}
