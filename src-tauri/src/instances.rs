use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use uuid::Uuid;

use crate::paths;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    pub profile: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_played: Option<String>,
    #[serde(default)]
    pub playtime_seconds: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InstanceFile {
    pub instances: Vec<Instance>,
}

fn file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(paths::app_dir(app)?.join("instances.json"))
}

fn load(app: &AppHandle) -> Result<InstanceFile, String> {
    let path = file(app)?;
    if !path.exists() {
        let seed = InstanceFile {
            instances: default_instances(),
        };
        paths::write_json(&path, &seed)?;
        for inst in &seed.instances {
            let dir = paths::instances_dir(app)?.join(&inst.id);
            std::fs::create_dir_all(dir.join("saves")).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(dir.join("mods")).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(dir.join("resourcepacks")).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(dir.join("shaderpacks")).map_err(|e| e.to_string())?;
        }
        return Ok(seed);
    }
    paths::read_json(&path)
}

fn save(app: &AppHandle, data: &InstanceFile) -> Result<(), String> {
    paths::write_json(&file(app)?, data)
}

fn default_instances() -> Vec<Instance> {
    vec![
        Instance {
            id: "perf-121".into(),
            name: "Performance 1.21.1".into(),
            minecraft_version: "1.21.1".into(),
            loader: "fabric".into(),
            loader_version: None,
            profile: "sodium-iris".into(),
            created_at: Utc::now().to_rfc3339(),
            last_played: None,
            playtime_seconds: 0,
        },
        Instance {
            id: "vanilla-120".into(),
            name: "Vanilla 1.20.1".into(),
            minecraft_version: "1.20.1".into(),
            loader: "vanilla".into(),
            loader_version: None,
            profile: "balanced".into(),
            created_at: Utc::now().to_rfc3339(),
            last_played: None,
            playtime_seconds: 0,
        },
        Instance {
            id: "legacy-189".into(),
            name: "PvP 1.8.9".into(),
            minecraft_version: "1.8.9".into(),
            loader: "vanilla".into(),
            loader_version: None,
            profile: "optifine-legacy".into(),
            created_at: Utc::now().to_rfc3339(),
            last_played: None,
            playtime_seconds: 0,
        },
    ]
}

#[tauri::command]
pub fn list_instances(app: AppHandle) -> Result<Vec<Instance>, String> {
    Ok(load(&app)?.instances)
}

#[tauri::command]
pub fn upsert_instance(app: AppHandle, mut instance: Instance) -> Result<Instance, String> {
    let mut data = load(&app)?;
    if instance.id.is_empty() {
        instance.id = Uuid::new_v4().to_string();
        instance.created_at = Utc::now().to_rfc3339();
    }
    let dir = paths::instances_dir(&app)?.join(&instance.id);
    std::fs::create_dir_all(dir.join("saves")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("mods")).map_err(|e| e.to_string())?;
    if let Some(existing) = data.instances.iter_mut().find(|i| i.id == instance.id) {
        *existing = instance.clone();
    } else {
        data.instances.push(instance.clone());
    }
    save(&app, &data)?;
    Ok(instance)
}

#[tauri::command]
pub fn delete_instance(app: AppHandle, id: String) -> Result<(), String> {
    let mut data = load(&app)?;
    data.instances.retain(|i| i.id != id);
    save(&app, &data)?;
    Ok(())
}

#[tauri::command]
pub fn touch_instance(app: AppHandle, id: String, add_seconds: u64) -> Result<(), String> {
    let mut data = load(&app)?;
    if let Some(i) = data.instances.iter_mut().find(|i| i.id == id) {
        i.last_played = Some(Utc::now().to_rfc3339());
        i.playtime_seconds += add_seconds;
    }
    save(&app, &data)
}

#[tauri::command]
pub fn instance_dir(app: AppHandle, id: String) -> Result<String, String> {
    Ok(paths::instances_dir(&app)?.join(id).to_string_lossy().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldSave {
    pub name: String,
    pub path: String,
    pub modified_at: Option<u64>,
}

#[tauri::command]
pub fn list_worlds(app: AppHandle, instance_id: String) -> Result<Vec<WorldSave>, String> {
    let dir = paths::instances_dir(&app)?.join(instance_id).join("saves");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut worlds = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            worlds.push(WorldSave {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().to_string_lossy().to_string(),
                modified_at: modified,
            });
        }
    }
    worlds.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(worlds)
}
