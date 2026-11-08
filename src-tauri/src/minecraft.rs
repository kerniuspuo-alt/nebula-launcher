use serde::{Deserialize, Serialize};
use sha2::Digest;
use sha1::Sha1;
use tauri::AppHandle;

use crate::paths;

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Serialize, Deserialize, Clone)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub sha1: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionInfo>,
}

#[tauri::command]
pub async fn fetch_version_manifest(app: AppHandle) -> Result<VersionManifest, String> {
    let manifest: VersionManifest = reqwest::get(MANIFEST_URL)
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let cache = paths::meta_dir(&app)?.join("version_manifest.json");
    paths::write_json(&cache, &manifest)?;
    Ok(manifest)
}

#[tauri::command]
pub async fn fetch_version_json(app: AppHandle, id: String) -> Result<serde_json::Value, String> {
    let manifest = fetch_version_manifest(app.clone()).await?;
    let info = manifest
        .versions
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("Unknown version {id}"))?;
    let json: serde_json::Value = reqwest::get(&info.url)
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let path = paths::versions_dir(&app)?.join(&id).join(format!("{id}.json"));
    paths::write_json(&path, &json)?;
    Ok(json)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceProfile {
    pub id: String,
    pub name: String,
    pub loader: String,
    pub minecraft: String,
    pub mods: Vec<ModHint>,
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModHint {
    pub slug: String,
    pub name: String,
    pub why: String,
}

#[tauri::command]
pub fn performance_profiles() -> Vec<PerformanceProfile> {
    vec![
        PerformanceProfile {
            id: "sodium-iris".into(),
            name: "Sodium + Iris".into(),
            loader: "fabric".into(),
            minecraft: "1.21.1".into(),
            mods: vec![
                ModHint { slug: "sodium".into(), name: "Sodium".into(), why: "Modern rendering; largest FPS gain.".into() },
                ModHint { slug: "iris".into(), name: "Iris".into(), why: "Shader support on Fabric.".into() },
                ModHint { slug: "lithium".into(), name: "Lithium".into(), why: "Server/simulation tick optimizations.".into() },
                ModHint { slug: "starlight".into(), name: "Starlight".into(), why: "Faster light engine.".into() },
                ModHint { slug: "ferrite-core".into(), name: "FerriteCore".into(), why: "Lower memory use.".into() },
                ModHint { slug: "modmenu".into(), name: "Mod Menu".into(), why: "In-game mod list.".into() },
            ],
            notes: "Best default for 1.20+ on laptops and Apple Silicon.".into(),
        },
        PerformanceProfile {
            id: "balanced".into(),
            name: "Vanilla balanced".into(),
            loader: "vanilla".into(),
            minecraft: "1.20.1".into(),
            mods: vec![],
            notes: "Official jar only. Use when you need unmodified multiplayer.".into(),
        },
        PerformanceProfile {
            id: "optifine-legacy".into(),
            name: "Legacy 1.8.9".into(),
            loader: "vanilla".into(),
            minecraft: "1.8.9".into(),
            mods: vec![],
            notes: "OptiFine is not redistributed. Download it from optifine.net and drop it into the instance mods/versions folder if you want it.".into(),
        },
    ]
}

#[tauri::command]
pub async fn search_modrinth(query: String, facets: Option<String>) -> Result<serde_json::Value, String> {
    let q = urlencoding::encode(&query);
    let mut url = format!("https://api.modrinth.com/v2/search?limit=20&query={q}");
    if let Some(f) = facets {
        url.push_str(&format!("&facets={}", urlencoding::encode(&f)));
    }
    let client = reqwest::Client::builder()
        .user_agent("nebula-launcher/0.1 (legitimate launcher)")
        .build()
        .map_err(|e| e.to_string())?;
    client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn modrinth_versions(
    project_id: String,
    minecraft_version: String,
    loader: String,
) -> Result<Vec<serde_json::Value>, String> {
    if project_id.is_empty()
        || !project_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Invalid Modrinth project id".into());
    }
    if !["fabric", "forge", "neoforge", "quilt"].contains(&loader.as_str()) {
        return Err("Choose a Fabric, Forge, NeoForge, or Quilt instance to install mods".into());
    }

    let game_versions_json = serde_json::json!([minecraft_version]).to_string();
    let loaders_json = serde_json::json!([loader]).to_string();
    let game_versions = urlencoding::encode(&game_versions_json);
    let loaders = urlencoding::encode(&loaders_json);
    let url = format!(
        "https://api.modrinth.com/v2/project/{project_id}/version?game_versions={game_versions}&loaders={loaders}"
    );
    let client = reqwest::Client::builder()
        .user_agent("nebula-launcher/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

fn curseforge_loader_type(loader: &str) -> Result<u8, String> {
    match loader {
        "forge" => Ok(1),
        "fabric" => Ok(4),
        "quilt" => Ok(5),
        "neoforge" => Ok(6),
        _ => Err("Choose a modded instance before searching CurseForge".into()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeHit {
    id: u64,
    name: String,
    slug: String,
    summary: String,
    download_count: u64,
    logo_url: Option<String>,
}

fn curseforge_client(api_key: &str) -> Result<reqwest::Client, String> {
    let api_key = api_key.trim();
    if api_key.len() < 20 || api_key.len() > 128 || !api_key.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte)) {
        return Err("Enter a valid CurseForge API key".into());
    }
    reqwest::Client::builder()
        .user_agent("Nebula-Launcher/0.1")
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("x-api-key", reqwest::header::HeaderValue::from_str(api_key).map_err(|error| error.to_string())?);
            headers
        })
        .build()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn search_curseforge_mods(
    query: String,
    minecraft_version: String,
    loader: String,
    api_key: String,
) -> Result<Vec<CurseForgeHit>, String> {
    let loader_type = curseforge_loader_type(&loader)?;
    let client = curseforge_client(&api_key)?;
    let result: serde_json::Value = client
        .get("https://api.curseforge.com/v1/mods/search")
        .query(&[
            ("gameId", "432"),
            ("classId", "6"),
            ("searchFilter", query.as_str()),
            ("sortField", "6"),
            ("sortOrder", "desc"),
            ("pageSize", "30"),
            ("index", "0"),
            ("gameVersion", minecraft_version.as_str()),
        ])
        .query(&[("modLoaderType", loader_type)])
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| format!("CurseForge search failed: {error}"))?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let hits = result
        .get("data")
        .and_then(|data| data.as_array())
        .ok_or("CurseForge returned an invalid search response")?;
    Ok(hits
        .iter()
        .filter_map(|hit| {
            Some(CurseForgeHit {
                id: hit.get("id")?.as_u64()?,
                name: hit.get("name")?.as_str()?.to_string(),
                slug: hit.get("slug").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
                summary: hit.get("summary").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
                download_count: hit.get("downloadCount").and_then(|value| value.as_u64()).unwrap_or_default(),
                logo_url: hit.pointer("/logo/url").and_then(|value| value.as_str()).map(str::to_string),
            })
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeFile {
    id: u64,
    file_name: String,
}

#[tauri::command]
pub async fn curseforge_mod_files(
    mod_id: u64,
    minecraft_version: String,
    loader: String,
    api_key: String,
) -> Result<Vec<CurseForgeFile>, String> {
    let loader_type = curseforge_loader_type(&loader)?;
    let client = curseforge_client(&api_key)?;
    let result: serde_json::Value = client
        .get(format!("https://api.curseforge.com/v1/mods/{mod_id}/files"))
        .query(&[
            ("gameVersion", minecraft_version.as_str()),
            ("modLoaderType", loader_type.to_string().as_str()),
            ("pageSize", "20".to_string().as_str()),
            ("index", "0".to_string().as_str()),
        ])
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| format!("CurseForge version lookup failed: {error}"))?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let files = result
        .get("data")
        .and_then(|data| data.as_array())
        .ok_or("CurseForge returned an invalid files response")?;
    Ok(files
        .iter()
        .filter_map(|file| {
            Some(CurseForgeFile {
                id: file.get("id")?.as_u64()?,
                file_name: file.get("fileName")?.as_str()?.to_string(),
            })
        })
        .collect())
}

#[tauri::command]
pub async fn install_curseforge_file(
    app: AppHandle,
    instance_id: String,
    file_id: u64,
    api_key: String,
) -> Result<String, String> {
    let instance = crate::instances::list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.id == instance_id)
        .ok_or("Instance not found")?;
    let client = curseforge_client(&api_key)?;
    let result: serde_json::Value = client
        .get(format!("https://api.curseforge.com/v1/mods/files/{file_id}"))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| format!("CurseForge file lookup failed: {error}"))?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let file = result.get("data").ok_or("CurseForge file metadata is missing")?;
    let compatible = file
        .get("gameVersions")
        .and_then(|versions| versions.as_array())
        .is_some_and(|versions| versions.iter().any(|version| version.as_str() == Some(&instance.minecraft_version)));
    if !compatible {
        return Err("CurseForge file does not match this Minecraft version".into());
    }
    let filename = file.get("fileName").and_then(|name| name.as_str()).ok_or("CurseForge file has no filename")?;
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| *name == filename && name.to_ascii_lowercase().ends_with(".jar"))
        .ok_or("CurseForge returned an invalid JAR filename")?;
    let download_url = file.get("downloadUrl").and_then(|url| url.as_str()).ok_or("CurseForge file has no download URL")?;
    let url = reqwest::Url::parse(download_url).map_err(|error| error.to_string())?;
    if url.scheme() != "https" || !url.host_str().is_some_and(|host| host.ends_with(".forgecdn.net")) {
        return Err("CurseForge returned an unexpected download host".into());
    }
    let expected_sha1 = file
        .get("hashes")
        .and_then(|hashes| hashes.as_array())
        .and_then(|hashes| hashes.iter().find(|hash| hash.get("algo").and_then(|algo| algo.as_u64()) == Some(1)))
        .and_then(|hash| hash.get("value").and_then(|value| value.as_str()))
        .ok_or("CurseForge did not provide a SHA-1 integrity hash")?;
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response.content_length().is_some_and(|length| length > 256 * 1024 * 1024) {
        return Err("Mod file exceeds the 256 MiB download limit".into());
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("Mod file exceeds the 256 MiB download limit".into());
    }
    if !hex::encode(Sha1::digest(&bytes)).eq_ignore_ascii_case(expected_sha1) {
        return Err("Downloaded CurseForge mod failed its SHA-1 integrity check".into());
    }
    let dest = crate::paths::instances_dir(&app)?.join(instance_id).join("mods").join(safe_name);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&dest, bytes).map_err(|error| error.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn download_modrinth_version(
    app: AppHandle,
    instance_id: String,
    version_id: String,
) -> Result<String, String> {
    if !valid_modrinth_id(&version_id) {
        return Err("Invalid Modrinth version id".into());
    }
    let instance = crate::instances::list_instances(app.clone())?
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;
    if !["fabric", "forge", "neoforge", "quilt"].contains(&instance.loader.as_str()) {
        return Err("Select a modded instance before installing mods".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("nebula-launcher/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let mut pending = vec![(version_id.clone(), false)];
    let mut scheduled = std::collections::HashSet::new();
    let mut versions = std::collections::HashMap::new();
    let mut install_order = Vec::new();

    while let Some((current_id, expanded)) = pending.pop() {
        if expanded {
            install_order.push(current_id);
            continue;
        }
        if !scheduled.insert(current_id.clone()) {
            continue;
        }
        if scheduled.len() > 32 {
            return Err("Mod dependency graph exceeds the 32-version install limit".into());
        }
        let version_url = format!("https://api.modrinth.com/v2/version/{current_id}");
        let json: serde_json::Value = client
            .get(version_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let game_matches = json
            .get("game_versions")
            .and_then(|versions| versions.as_array())
            .is_some_and(|versions| versions.iter().any(|version| version.as_str() == Some(&instance.minecraft_version)));
        let loader_matches = json
            .get("loaders")
            .and_then(|loaders| loaders.as_array())
            .is_some_and(|loaders| loaders.iter().any(|loader| loader.as_str() == Some(&instance.loader)));
        if !game_matches || !loader_matches {
            return Err(format!("Modrinth version {current_id} is incompatible with this instance"));
        }

        let mut dependencies = Vec::new();
        if let Some(items) = json.get("dependencies").and_then(|items| items.as_array()) {
            for dependency in items {
                if dependency.get("dependency_type").and_then(|kind| kind.as_str()) != Some("required") {
                    continue;
                }
                let dependency_id = if let Some(id) = dependency.get("version_id").and_then(|id| id.as_str()) {
                    id.to_string()
                } else if let Some(project_id) = dependency.get("project_id").and_then(|id| id.as_str()) {
                    if !valid_modrinth_id(project_id) {
                        return Err("Modrinth returned an invalid dependency project id".into());
                    }
                    compatible_project_version(
                        &client,
                        project_id,
                        &instance.minecraft_version,
                        &instance.loader,
                    )
                    .await?
                } else {
                    return Err("Modrinth required dependency has no project or version id".into());
                };
                if !valid_modrinth_id(&dependency_id) {
                    return Err("Modrinth returned an invalid dependency version id".into());
                }
                dependencies.push(dependency_id);
            }
        }
        versions.insert(current_id.clone(), json);
        pending.push((current_id, true));
        for dependency_id in dependencies.into_iter().rev() {
            if !scheduled.contains(&dependency_id) {
                pending.push((dependency_id, false));
            }
        }
    }

    let mut root_path = None;
    for install_id in install_order {
        let json = versions
            .remove(&install_id)
            .ok_or("Missing resolved Modrinth version metadata")?;
        let path = download_modrinth_file(&client, &app, &instance_id, &json).await?;
        if install_id == version_id {
            root_path = Some(path);
        }
    }
    root_path.ok_or_else(|| "Requested Modrinth version was not installed".into())
}

#[cfg(test)]
mod curseforge_tests {
    use super::curseforge_loader_type;

    #[test]
    fn maps_supported_minecraft_loaders_to_curseforge_ids() {
        assert_eq!(curseforge_loader_type("forge").unwrap(), 1);
        assert_eq!(curseforge_loader_type("fabric").unwrap(), 4);
        assert_eq!(curseforge_loader_type("quilt").unwrap(), 5);
        assert_eq!(curseforge_loader_type("neoforge").unwrap(), 6);
        assert!(curseforge_loader_type("vanilla").is_err());
    }
}

fn valid_modrinth_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

async fn compatible_project_version(
    client: &reqwest::Client,
    project_id: &str,
    minecraft_version: &str,
    loader: &str,
) -> Result<String, String> {
    let game_versions_json = serde_json::json!([minecraft_version]).to_string();
    let loaders_json = serde_json::json!([loader]).to_string();
    let game_versions = urlencoding::encode(&game_versions_json);
    let loaders = urlencoding::encode(&loaders_json);
    let url = format!(
        "https://api.modrinth.com/v2/project/{project_id}/version?game_versions={game_versions}&loaders={loaders}"
    );
    let versions: Vec<serde_json::Value> = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    versions
        .first()
        .and_then(|version| version.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("No compatible version found for required dependency {project_id}"))
}

async fn download_modrinth_file(
    client: &reqwest::Client,
    app: &AppHandle,
    instance_id: &str,
    json: &serde_json::Value,
) -> Result<String, String> {
    let file = json
        .get("files")
        .and_then(|files| files.as_array())
        .and_then(|files| files.iter().find(|file| file.get("primary").and_then(|primary| primary.as_bool()).unwrap_or(false)).or(files.first()))
        .ok_or("No file on Modrinth version")?;
    let dl = file.get("url").and_then(|url| url.as_str()).ok_or("Missing file url")?;
    let download_url = reqwest::Url::parse(dl).map_err(|e| e.to_string())?;
    if download_url.scheme() != "https" || download_url.host_str() != Some("cdn.modrinth.com") {
        return Err("Modrinth returned an unexpected file host".into());
    }
    let filename = file
        .get("filename")
        .and_then(|u| u.as_str())
        .unwrap_or("mod.jar");
    let safe_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| *name == filename && name.to_ascii_lowercase().ends_with(".jar"))
        .ok_or("Modrinth returned an invalid JAR filename")?;
    let expected_sha512 = file
        .pointer("/hashes/sha512")
        .and_then(|hash| hash.as_str())
        .ok_or("Modrinth did not provide a SHA-512 hash")?;
    let response = client.get(download_url).send().await.map_err(|e| e.to_string())?;
    let response = response.error_for_status().map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|length| length > 256 * 1024 * 1024) {
        return Err("Mod file exceeds the 256 MiB download limit".into());
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("Mod file exceeds the 256 MiB download limit".into());
    }
    let actual_sha512 = hex::encode(sha2::Sha512::digest(&bytes));
    if !actual_sha512.eq_ignore_ascii_case(expected_sha512) {
        return Err("Downloaded mod failed its SHA-512 integrity check".into());
    }
    let dest = crate::paths::instances_dir(app)?
        .join(instance_id)
        .join("mods")
        .join(safe_name);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}
