use serde::Serialize;
use futures_util::{stream, StreamExt};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use crate::auth;
use crate::instances;
use crate::jvm;
use crate::paths;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub stage: String,
    pub current: u64,
    pub total: u64,
    pub detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub pid: u32,
    pub command: String,
}

fn emit(app: &AppHandle, stage: &str, current: u64, total: u64, detail: &str) {
    let _ = app.emit(
        "install-progress",
        InstallProgress {
            stage: stage.into(),
            current,
            total,
            detail: detail.into(),
        },
    );
}

async fn download_file(url: &str, dest: &std::path::Path) -> Result<(), String> {
    download_file_with_client(&reqwest::Client::new(), url, dest).await
}

async fn download_file_with_client(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let mut tmp = dest.to_path_buf();
    tmp.set_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dest).map_err(|e| e.to_string())?;
    Ok(())
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "osx",
        "windows" => "windows",
        _ => "linux",
    }
}

fn rules_allow(rules: &serde_json::Value) -> bool {
    let Some(arr) = rules.as_array() else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    let mut allow = false;
    for rule in arr {
        let action = rule.get("action").and_then(|a| a.as_str()).unwrap_or("");
        let os_ok = match rule.pointer("/os/name").and_then(|n| n.as_str()) {
            Some(name) => name == os_name(),
            None => true,
        };
        if os_ok {
            allow = action == "allow";
        }
    }
    allow
}

#[tauri::command]
pub async fn install_version(app: AppHandle, version_id: String) -> Result<String, String> {
    let json_path = paths::versions_dir(&app)?.join(&version_id).join(format!("{version_id}.json"));
    let json = if json_path.exists() {
        paths::read_json(&json_path)?
    } else {
        emit(&app, "meta", 0, 1, "Fetching version JSON");
        crate::minecraft::fetch_version_json(app.clone(), version_id.clone()).await?
    };
    let client = json
        .pointer("/downloads/client/url")
        .and_then(|u| u.as_str())
        .ok_or("Version JSON missing client download")?;
    let jar = paths::versions_dir(&app)?.join(&version_id).join(format!("{version_id}.jar"));
    if !jar.exists() {
        emit(&app, "client", 0, 1, "Downloading client jar");
        download_file(client, &jar).await?;
    }

    let libs = json.get("libraries").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let total = libs.len() as u64;
    for (i, lib) in libs.iter().enumerate() {
        if let Some(rules) = lib.get("rules") {
            if !rules_allow(rules) {
                continue;
            }
        }
        if let Some(url) = lib.pointer("/downloads/artifact/url").and_then(|u| u.as_str()) {
            let path = lib
                .pointer("/downloads/artifact/path")
                .and_then(|p| p.as_str())
                .unwrap_or("lib.jar");
            let dest = paths::libraries_dir(&app)?.join(path);
            emit(&app, "libraries", i as u64, total, path);
            download_file(url, &dest).await?;
        }
        // natives
        let nkey = match std::env::consts::OS {
            "macos" => {
                if std::env::consts::ARCH == "aarch64" {
                    "natives-macos-arm64"
                } else {
                    "natives-osx"
                }
            }
            "windows" => "natives-windows",
            _ => "natives-linux",
        };
        if let Some(natives) = lib.get("downloads").and_then(|d| d.get("classifiers")) {
            if let Some(art) = natives.get(nkey).or_else(|| {
                if nkey.contains("macos") {
                    natives.get("natives-macos").or_else(|| natives.get("natives-osx"))
                } else {
                    None
                }
            }) {
                if let Some(url) = art.get("url").and_then(|u| u.as_str()) {
                    let path = art.get("path").and_then(|p| p.as_str()).unwrap_or("natives.jar");
                    let dest = paths::libraries_dir(&app)?.join(path);
                    download_file(url, &dest).await?;
                }
            }
        }
    }

    if let Some(asset_index) = json.pointer("/assetIndex/url").and_then(|u| u.as_str()) {
        let id = json
            .pointer("/assetIndex/id")
            .and_then(|i| i.as_str())
            .unwrap_or("legacy");
        let index_path = paths::assets_dir(&app)?.join("indexes").join(format!("{id}.json"));
        let index: serde_json::Value = if index_path.exists() {
            paths::read_json(&index_path)?
        } else {
            emit(&app, "assets", 0, 1, "Downloading asset index");
            let index: serde_json::Value = reqwest::get(asset_index)
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            paths::write_json(&index_path, &index)?;
            index
        };
        let objects = index.get("objects").cloned().unwrap_or(serde_json::json!({}));
        let map = objects.as_object().cloned().unwrap_or_default();
        let mut missing_assets = Vec::new();
        for (_name, obj) in map.iter() {
            let hash = obj.get("hash").and_then(|h| h.as_str()).unwrap_or("");
            if hash.len() < 2 {
                continue;
            }
            let prefix = &hash[..2];
            let dest = paths::assets_dir(&app)?.join("objects").join(prefix).join(hash);
            if dest.exists() {
                continue;
            }
            let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
            missing_assets.push((hash.to_string(), url, dest));
        }
        let pending_total = missing_assets.len() as u64;
        let completed = Arc::new(AtomicU64::new(0));
        let client = reqwest::Client::new();
        let app_handle = app.clone();
        let downloaded = stream::iter(missing_assets.into_iter().map(|(hash, url, dest)| {
            let app = app_handle.clone();
            let client = client.clone();
            let completed = completed.clone();
            async move {
                if let Err(error) = download_file_with_client(&client, &url, &dest).await {
                    eprintln!("asset {hash}: {error}");
                }
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 25 == 0 || count == pending_total {
                    emit(&app, "assets", count, pending_total, &hash);
                }
            }
        }))
        .buffer_unordered(12)
        .collect::<Vec<_>>()
        .await;
        if !downloaded.is_empty() {
            emit(&app, "assets", pending_total, pending_total, "Asset downloads complete");
        }
    }

    emit(&app, "done", 1, 1, "Ready");
    Ok(jar.to_string_lossy().to_string())
}

fn maven_artifact_path(coordinate: &str) -> Result<String, String> {
    let (coordinate, extension) = coordinate.split_once('@').unwrap_or((coordinate, "jar"));
    if !extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Invalid Maven artifact extension".into());
    }
    let parts = coordinate.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)))
    {
        return Err("Invalid Maven artifact coordinate".into());
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).map(|value| format!("-{value}")).unwrap_or_default();
    Ok(format!("{group}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}"))
}

async fn install_loader_profile(
    app: &AppHandle,
    minecraft_version: &str,
    loader: &str,
    loader_version: Option<&str>,
) -> Result<(String, String), String> {
    let client = reqwest::Client::builder()
        .user_agent("nebula-launcher/0.1")
        .build()
        .map_err(|error| error.to_string())?;
    let loader_version = if let Some(version) = loader_version {
        version.to_string()
    } else {
        let endpoint = match loader {
            "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft_version}"),
            "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}"),
            _ => return Err(format!("Unknown loader {loader}")),
        };
        let versions: Vec<serde_json::Value> = client
            .get(endpoint)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .await
            .map_err(|error| error.to_string())?;
        versions
            .iter()
            .find(|entry| entry.pointer("/loader/stable").and_then(|stable| stable.as_bool()) == Some(true))
            .or_else(|| versions.first())
            .and_then(|entry| entry.pointer("/loader/version").and_then(|value| value.as_str()))
            .ok_or_else(|| format!("No {loader} loader is available for Minecraft {minecraft_version}"))?
            .to_string()
    };
    if !loader_version
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
    {
        return Err("Invalid Fabric loader version".into());
    }

    let profile_id = format!("{loader}-loader-{loader_version}-{minecraft_version}");
    let profile_path = paths::versions_dir(app)?
        .join(&profile_id)
        .join(format!("{profile_id}.json"));
    if profile_path.exists() {
        return Ok((loader_version, profile_id));
    }

    let endpoint = match loader {
        "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft_version}/{loader_version}/profile/json"),
        "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}/{loader_version}/profile/json"),
        _ => return Err(format!("Unknown loader {loader}")),
    };
    let mut profile: serde_json::Value = client
        .get(endpoint)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    profile["id"] = serde_json::Value::String(profile_id.clone());
    profile["inheritsFrom"] = serde_json::Value::String(minecraft_version.into());

    let profile_libraries = profile
        .get("libraries")
        .and_then(|libraries| libraries.as_array())
        .cloned()
        .unwrap_or_default();
    let total = profile_libraries.len() as u64;
    for (index, library) in profile_libraries.iter().enumerate() {
        if !rules_allow(library.get("rules").unwrap_or(&serde_json::Value::Null)) {
            continue;
        }
        let coordinate = library
            .get("name")
            .and_then(|name| name.as_str())
            .ok_or("Fabric library missing Maven coordinate")?;
        let path = library
            .pointer("/downloads/artifact/path")
            .and_then(|path| path.as_str())
            .map(str::to_string)
            .map(Ok)
            .unwrap_or_else(|| maven_artifact_path(coordinate))?;
        let url = if let Some(url) = library.pointer("/downloads/artifact/url").and_then(|url| url.as_str()) {
            url.to_string()
        } else {
            let base = library
                .get("url")
                .and_then(|url| url.as_str())
                .ok_or("Fabric library missing Maven repository URL")?;
            format!("{}/{}", base.trim_end_matches('/'), path)
        };
        let parsed_url = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
        if parsed_url.scheme() != "https"
            || !matches!(parsed_url.host_str(), Some("maven.fabricmc.net" | "maven.quiltmc.org" | "libraries.minecraft.net" | "repo.maven.apache.org"))
        {
            return Err(format!("{loader} library has an untrusted repository URL: {url}"));
        }
        emit(app, "fabric-libraries", index as u64, total, coordinate);
        download_file(&url, &paths::libraries_dir(app)?.join(path)).await?;
    }

    let base_path = paths::versions_dir(app)?
        .join(minecraft_version)
        .join(format!("{minecraft_version}.json"));
    let mut base: serde_json::Value = paths::read_json(&base_path)?;
    let mut libraries = base
        .get("libraries")
        .and_then(|libraries| libraries.as_array())
        .cloned()
        .unwrap_or_default();
    let existing = libraries
        .iter()
        .filter_map(|library| library.get("name").and_then(|name| name.as_str()).map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    libraries.extend(profile_libraries.into_iter().filter(|library| {
        library
            .get("name")
            .and_then(|name| name.as_str())
            .is_some_and(|name| !existing.contains(name))
    }));
    base["id"] = serde_json::Value::String(profile_id.clone());
    base["mainClass"] = profile
        .get("mainClass")
        .cloned()
        .ok_or("Fabric profile missing main class")?;
    base["libraries"] = serde_json::Value::Array(libraries);
    if let Some(profile_args) = profile.get("arguments").and_then(|args| args.as_object()) {
        for key in ["game", "jvm"] {
            if let Some(extra) = profile_args.get(key).and_then(|args| args.as_array()) {
                let target = base["arguments"][key].as_array_mut();
                if let Some(target) = target {
                    target.extend(extra.iter().cloned());
                } else {
                    base["arguments"][key] = serde_json::Value::Array(extra.clone());
                }
            }
        }
    }
    paths::write_json(&profile_path, &base)?;
    emit(app, loader, 1, 1, &format!("{loader} profile ready"));
    Ok((loader_version, profile_id))
}

fn compare_loader_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = left
        .split('-')
        .next()
        .unwrap_or(left)
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect::<Vec<_>>();
    let right_parts = right
        .split('-')
        .next()
        .unwrap_or(right)
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect::<Vec<_>>();
    left_parts.cmp(&right_parts).then_with(|| {
        let left_stable = !left.contains('-');
        let right_stable = !right.contains('-');
        left_stable.cmp(&right_stable)
    })
}

fn neoforge_line(minecraft_version: &str) -> Option<String> {
    let mut parts = minecraft_version.strip_prefix("1.")?.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}

fn compose_installer_profile(
    mut base: serde_json::Value,
    profile: &serde_json::Value,
    minecraft_version: &str,
    profile_id: &str,
) -> Result<serde_json::Value, String> {
    let profile_libraries = profile
        .get("libraries")
        .and_then(|libraries| libraries.as_array())
        .cloned()
        .unwrap_or_default();
    let mut libraries = base
        .get("libraries")
        .and_then(|libraries| libraries.as_array())
        .cloned()
        .unwrap_or_default();
    let mut names = libraries
        .iter()
        .filter_map(|library| library.get("name").and_then(|name| name.as_str()).map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    libraries.extend(profile_libraries.into_iter().filter(|library| {
        library
            .get("name")
            .and_then(|name| name.as_str())
            .is_some_and(|name| names.insert(name.to_string()))
    }));
    base["id"] = serde_json::Value::String(profile_id.into());
    base["inheritsFrom"] = serde_json::Value::String(minecraft_version.into());
    base["libraries"] = serde_json::Value::Array(libraries);
    if let Some(main_class) = profile.get("mainClass") {
        base["mainClass"] = main_class.clone();
    }
    if let (Some(base_args), Some(profile_args)) = (
        base.get_mut("arguments").and_then(|arguments| arguments.as_object_mut()),
        profile.get("arguments").and_then(|arguments| arguments.as_object()),
    ) {
        for key in ["game", "jvm"] {
            if let Some(extra) = profile_args.get(key).and_then(|arguments| arguments.as_array()) {
                base_args
                    .entry(key.to_string())
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                    .as_array_mut()
                    .ok_or_else(|| format!("Invalid {key} argument list in loader profile"))?
                    .extend(extra.iter().cloned());
            }
        }
    } else if let Some(profile_args) = profile.get("arguments") {
        base["arguments"] = profile_args.clone();
    }
    if let Some(profile_args) = profile.get("minecraftArguments").and_then(|args| args.as_str()) {
        let base_args = base.get("minecraftArguments").and_then(|args| args.as_str()).unwrap_or("");
        base["minecraftArguments"] = serde_json::Value::String(format!("{base_args} {profile_args}").trim().to_string());
    }
    Ok(base)
}

fn merge_installer_profile(app: &AppHandle, minecraft_version: &str, profile_id: &str) -> Result<(), String> {
    let profile_path = paths::versions_dir(app)?
        .join(profile_id)
        .join(format!("{profile_id}.json"));
    let profile: serde_json::Value = paths::read_json(&profile_path)?;
    let base_path = paths::versions_dir(app)?
        .join(minecraft_version)
        .join(format!("{minecraft_version}.json"));
    let base: serde_json::Value = paths::read_json(&base_path)?;
    let merged = compose_installer_profile(base, &profile, minecraft_version, profile_id)?;
    paths::write_json(&profile_path, &merged)
}

async fn install_classic_loader(
    app: &AppHandle,
    minecraft_version: &str,
    loader: &str,
    requested_version: Option<&str>,
) -> Result<(String, String), String> {
    if !minecraft_version
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err("Invalid Minecraft version for loader installation".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("nebula-launcher/0.1")
        .build()
        .map_err(|error| error.to_string())?;
    let loader_version = if let Some(version) = requested_version {
        version.to_string()
    } else if loader == "forge" {
        let promotions: serde_json::Value = client
            .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .await
            .map_err(|error| error.to_string())?;
        let promos = promotions.get("promos").ok_or("Forge promotions missing")?;
        promos
            .get(format!("{minecraft_version}-recommended"))
            .or_else(|| promos.get(format!("{minecraft_version}-latest")))
            .and_then(|version| version.as_str())
            .ok_or_else(|| format!("No Forge build available for Minecraft {minecraft_version}"))?
            .to_string()
    } else if loader == "neoforge" {
        let line = neoforge_line(minecraft_version)
            .ok_or_else(|| format!("No NeoForge releases available for Minecraft {minecraft_version}"))?;
        let metadata: serde_json::Value = client
            .get("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .await
            .map_err(|error| error.to_string())?;
        let mut versions = metadata
            .get("versions")
            .and_then(|versions| versions.as_array())
            .ok_or("NeoForge version list missing")?
            .iter()
            .filter_map(|version| version.as_str())
            .filter(|version| !version.contains('-'))
            .filter(|version| version.starts_with(&format!("{line}.")))
            .map(str::to_string)
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| compare_loader_versions(left, right));
        versions.pop().ok_or_else(|| format!("No NeoForge build available for Minecraft {minecraft_version}"))?
    } else {
        return Err(format!("Unknown loader {loader}"));
    };
    if !loader_version
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
    {
        return Err(format!("Invalid {loader} installer version"));
    }

    let (profile_id, installer_url, installer_flag) = match loader {
        "forge" => (
            format!("{minecraft_version}-forge-{loader_version}"),
            format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft_version}-{loader_version}/forge-{minecraft_version}-{loader_version}-installer.jar"),
            "--installClient",
        ),
        "neoforge" => (
            format!("neoforge-{loader_version}"),
            format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"),
            "--install-client",
        ),
        _ => return Err(format!("Unknown loader {loader}")),
    };
    let installer_url = reqwest::Url::parse(&installer_url).map_err(|error| error.to_string())?;
    let expected_host = if loader == "forge" { "maven.minecraftforge.net" } else { "maven.neoforged.net" };
    if installer_url.scheme() != "https" || installer_url.host_str() != Some(expected_host) {
        return Err(format!("Unexpected {loader} installer host"));
    }
    let app_data = paths::app_dir(app)?;
    let installer_path = app_data
        .join("loader-installers")
        .join(format!("{loader}-{loader_version}-installer.jar"));
    download_file(installer_url.as_str(), &installer_path).await?;
    let profile_path = paths::versions_dir(app)?
        .join(&profile_id)
        .join(format!("{profile_id}.json"));
    if !profile_path.exists() {
        let launcher_profiles = app_data.join("launcher_profiles.json");
        if !launcher_profiles.exists() {
            let now = chrono::Utc::now().to_rfc3339();
            paths::write_json(
                &launcher_profiles,
                &serde_json::json!({
                    "profiles": {
                        "Nebula": {
                            "name": "Nebula",
                            "type": "custom",
                            "created": now,
                            "lastUsed": now,
                            "lastVersionId": minecraft_version,
                            "gameDir": app_data
                        }
                    },
                    "settings": {}
                }),
            )?;
        }
        let plan = jvm::plan_jvm(app.clone(), minecraft_version.to_string(), None);
        let java = plan
            .matched
            .ok_or_else(|| format!("Forge-family installation requires Java {}+", plan.required_major))?
            .path;
        let output = Command::new(java)
            .arg("-jar")
            .arg(&installer_path)
            .arg(installer_flag)
            .arg(&app_data)
            .current_dir(&app_data)
            .output()
            .map_err(|error| format!("Could not start {loader} installer: {error}"))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(format!("{loader} installer failed: {}", detail.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")));
        }
        if !profile_path.exists() {
            return Err(format!("{loader} installer completed but did not create {profile_id}"));
        }
        merge_installer_profile(app, minecraft_version, &profile_id)?;
    }
    emit(app, loader, 1, 1, &format!("{loader} {loader_version} installed"));
    Ok((loader_version, profile_id))
}

#[tauri::command]
pub async fn install_instance(app: AppHandle, instance_id: String) -> Result<String, String> {
    let mut instance = instances::list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.id == instance_id)
        .ok_or("Instance not found")?;
    if !["vanilla", "fabric", "quilt", "forge", "neoforge"].contains(&instance.loader.as_str()) {
        return Err(format!("The {} loader installer is not implemented yet.", instance.loader));
    }
    let base_jar = install_version(app.clone(), instance.minecraft_version.clone()).await?;
    match instance.loader.as_str() {
        "vanilla" => Ok(base_jar),
        "fabric" | "quilt" => {
            let (loader_version, _) = install_loader_profile(
                &app,
                &instance.minecraft_version,
                &instance.loader,
                instance.loader_version.as_deref(),
            )
            .await?;
            instance.loader_version = Some(loader_version);
            instances::upsert_instance(app, instance)?;
            Ok(base_jar)
        }
        "forge" | "neoforge" => {
            let (loader_version, _) = install_classic_loader(
                &app,
                &instance.minecraft_version,
                &instance.loader,
                instance.loader_version.as_deref(),
            )
            .await?;
            instance.loader_version = Some(loader_version);
            instances::upsert_instance(app, instance)?;
            Ok(base_jar)
        }
        _ => unreachable!("loader validated before installation"),
    }
}

fn classpath(app: &AppHandle, json: &serde_json::Value, version_id: &str) -> Result<String, String> {
    let mut entries = Vec::new();
    let jar = paths::versions_dir(app)?.join(version_id).join(format!("{version_id}.jar"));
    entries.push(jar.to_string_lossy().to_string());
    if let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) {
        for lib in libs {
            if let Some(rules) = lib.get("rules") {
                if !rules_allow(rules) {
                    continue;
                }
            }
            let path = lib
                .pointer("/downloads/artifact/path")
                .and_then(|path| path.as_str())
                .map(str::to_string)
                .or_else(|| {
                    lib.get("name")
                        .and_then(|name| name.as_str())
                        .and_then(|coordinate| maven_artifact_path(coordinate).ok())
                });
            if let Some(path) = path {
                entries.push(paths::libraries_dir(app)?.join(path).to_string_lossy().to_string());
            }
        }
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    Ok(entries.join(sep))
}

fn extract_natives(app: &AppHandle, json: &serde_json::Value, natives_dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(natives_dir).map_err(|e| e.to_string())?;
    let complete = natives_dir.join(".nebula-ready");
    if complete.exists() {
        return Ok(());
    }
    let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) else {
        return Ok(());
    };
    for lib in libs {
        if let Some(classifiers) = lib.pointer("/downloads/classifiers") {
            for key in ["natives-macos-arm64", "natives-macos", "natives-osx", "natives-windows", "natives-linux"] {
                if std::env::consts::OS == "macos" && !key.contains("mac") && key != "natives-osx" {
                    continue;
                }
                if std::env::consts::OS == "windows" && key != "natives-windows" {
                    continue;
                }
                if let Some(path) = classifiers.pointer(&format!("/{key}/path")).and_then(|p| p.as_str()) {
                    let zip_path = paths::libraries_dir(app)?.join(path);
                    if !zip_path.exists() {
                        continue;
                    }
                    if let Ok(file) = std::fs::File::open(&zip_path) {
                        if let Ok(mut zip) = zip::ZipArchive::new(file) {
                            let _ = zip.extract(natives_dir);
                        }
                    }
                }
            }
        }
    }
    std::fs::write(complete, b"ready").map_err(|error| error.to_string())?;
    Ok(())
}

fn expand_argument(value: &str, variables: &std::collections::HashMap<&str, String>) -> String {
    variables.iter().fold(value.to_string(), |value, (key, replacement)| {
        value.replace(&format!("${{{key}}}"), replacement)
    })
}

fn append_version_arguments(
    target: &mut Vec<String>,
    source: Option<&serde_json::Value>,
    variables: &std::collections::HashMap<&str, String>,
) {
    let Some(values) = source.and_then(|source| source.as_array()) else {
        return;
    };
    for value in values {
        if let Some(argument) = value.as_str() {
            target.push(expand_argument(argument, variables));
            continue;
        }
        if !rules_allow(value.get("rules").unwrap_or(&serde_json::Value::Null)) {
            continue;
        }
        match value.get("value") {
            Some(serde_json::Value::String(argument)) => target.push(expand_argument(argument, variables)),
            Some(serde_json::Value::Array(arguments)) => {
                target.extend(arguments.iter().filter_map(|argument| argument.as_str()).map(|argument| expand_argument(argument, variables)));
            }
            _ => {}
        }
    }
}

fn game_arguments(
    json: &serde_json::Value,
    variables: &std::collections::HashMap<&str, String>,
) -> Vec<String> {
    if let Some(arguments) = json.pointer("/arguments/game") {
        let mut output = Vec::new();
        append_version_arguments(&mut output, Some(arguments), variables);
        return output;
    }
    json.get("minecraftArguments")
        .and_then(|arguments| arguments.as_str())
        .map(|arguments| {
            arguments
                .split_whitespace()
                .map(|argument| expand_argument(argument, variables))
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    instance_id: String,
    server_host: Option<String>,
    server_port: Option<u16>,
) -> Result<LaunchResult, String> {
    let mut inst = instances::list_instances(app.clone())?
        .into_iter()
        .find(|i| i.id == instance_id)
        .ok_or("Instance not found")?;
    let flagged_mods = crate::scanner::scan_instance_mods(app.clone(), inst.id.clone())?
        .into_iter()
        .filter(|finding| finding.blocked)
        .take(3)
        .map(|finding| {
            format!(
                "{} ({})",
                std::path::Path::new(&finding.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                finding.reasons.join(", ")
            )
        })
        .collect::<Vec<_>>();
    if !flagged_mods.is_empty() {
        return Err(format!(
            "Launch blocked: local heuristic scan flagged {}",
            flagged_mods.join("; ")
        ));
    }
    let account = auth::active_token(&app)?;
    if account.access_token == "<redacted>" || account.access_token.is_empty() {
        return Err("Sign in with Microsoft first.".into());
    }
    install_instance(app.clone(), inst.id.clone()).await?;
    inst = instances::list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.id == instance_id)
        .ok_or("Instance not found")?;
    let base_version_id = inst.minecraft_version.clone();
    let version_id = match inst.loader.as_str() {
        "vanilla" => base_version_id.clone(),
        "fabric" | "quilt" => {
            let loader_version = inst.loader_version.as_deref().ok_or("Fabric version was not installed")?;
            format!("{}-loader-{loader_version}-{base_version_id}", inst.loader)
        }
        "forge" => {
            let loader_version = inst.loader_version.as_deref().ok_or("Forge version was not installed")?;
            format!("{base_version_id}-forge-{loader_version}")
        }
        "neoforge" => {
            let loader_version = inst.loader_version.as_deref().ok_or("NeoForge version was not installed")?;
            format!("neoforge-{loader_version}")
        }
        loader => return Err(format!("The {loader} loader is not implemented yet.")),
    };
    let json_path = paths::versions_dir(&app)?.join(&version_id).join(format!("{version_id}.json"));
    let json: serde_json::Value = paths::read_json(&json_path)?;
    let plan = jvm::plan_jvm(app.clone(), base_version_id.clone(), None);
    let java = plan
        .matched
        .ok_or_else(|| {
            format!(
                "No Java {}+ runtime found. Use Settings to download Temurin.",
                plan.required_major
            )
        })?
        .path;
    let game_dir = paths::instances_dir(&app)?.join(&inst.id);
    let natives = game_dir.join("natives").join(&version_id);
    extract_natives(&app, &json, &natives)?;
    let cp = classpath(&app, &json, &base_version_id)?;
    let main_class = json
        .get("mainClass")
        .and_then(|m| m.as_str())
        .unwrap_or("net.minecraft.client.main.Main");

    let assets = paths::assets_dir(&app)?.to_string_lossy().to_string();
    let asset_index = json
        .pointer("/assetIndex/id")
        .and_then(|value| value.as_str())
        .unwrap_or(&base_version_id)
        .to_string();
    let mut variables = std::collections::HashMap::new();
    variables.insert("auth_player_name", account.name.clone());
    variables.insert("version_name", version_id.clone());
    variables.insert("game_directory", game_dir.to_string_lossy().to_string());
    variables.insert("assets_root", assets.clone());
    variables.insert("assets_index_name", asset_index.clone());
    variables.insert("auth_uuid", account.uuid.clone());
    variables.insert("auth_access_token", account.access_token.clone());
    variables.insert("clientid", account.client_id.clone());
    variables.insert("auth_xuid", String::new());
    variables.insert("user_type", "msa".into());
    variables.insert("version_type", json.get("type").and_then(|value| value.as_str()).unwrap_or("release").into());
    variables.insert("user_properties", "{}".into());
    variables.insert("auth_session", format!("token:{}:{}:{}", account.uuid, account.name, account.access_token));
    variables.insert("game_assets", asset_index);
    variables.insert("natives_directory", natives.to_string_lossy().to_string());
    variables.insert("launcher_name", "Nebula".into());
    variables.insert("launcher_version", "0.1.0".into());
    variables.insert("resolution_width", "854".into());
    variables.insert("resolution_height", "480".into());
    variables.insert("classpath", cp.clone());
    variables.insert("classpath_separator", if cfg!(windows) { ";" } else { ":" }.into());
    variables.insert("library_directory", paths::libraries_dir(&app)?.to_string_lossy().to_string());

    let mut args: Vec<String> = plan.flags;
    let jvm_arguments = json.pointer("/arguments/jvm");
    if jvm_arguments.is_some() {
        append_version_arguments(&mut args, jvm_arguments, &variables);
    } else {
        args.push(format!("-Djava.library.path={}", natives.to_string_lossy()));
        args.push("-cp".into());
        args.push(cp);
    }
    args.push(main_class.into());
    args.extend(game_arguments(&json, &variables));
    if let Some(host) = server_host {
        args.push("--server".into());
        args.push(host);
        args.push("--port".into());
        args.push(server_port.unwrap_or(25565).to_string());
    }

    let mut cmd = Command::new(&java);
    cmd.args(&args)
        .current_dir(&game_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    let _ = instances::touch_instance(app.clone(), inst.id.clone(), 0);
    let _ = crate::playtime::log_session(
        app,
        crate::playtime::Session {
            instance_id: inst.id,
            started_at: chrono::Utc::now().to_rfc3339(),
            ended_at: None,
            seconds: 0,
            kind: "launch".into(),
        },
    );
    Ok(LaunchResult {
        pid,
        command: format!("{java} … {main_class}"),
    })
}

#[tauri::command]
pub fn save_crosshair_png(app: AppHandle, data_url: String) -> Result<String, String> {
    let b64 = data_url.split(',').nth(1).ok_or("Invalid data URL")?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    let path = paths::app_dir(&app)?.join("crosshair.png");
    let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        compare_loader_versions, compose_installer_profile, game_arguments, maven_artifact_path,
        neoforge_line,
    };
    use std::collections::HashMap;

    #[test]
    fn resolves_maven_artifact_coordinates() {
        assert_eq!(
            maven_artifact_path("net.fabricmc:fabric-loader:0.18.1").unwrap(),
            "net/fabricmc/fabric-loader/0.18.1/fabric-loader-0.18.1.jar"
        );
        assert_eq!(
            maven_artifact_path("org.example:library:2.0:client@zip").unwrap(),
            "org/example/library/2.0/library-2.0-client.zip"
        );
    }

    #[test]
    fn rejects_invalid_maven_artifact_coordinates() {
        assert!(maven_artifact_path("../../escape:artifact:1.0").is_err());
        assert!(maven_artifact_path("group:artifact").is_err());
    }

    #[test]
    fn expands_modern_game_arguments() {
        let variables = HashMap::from([("auth_player_name", "Alex".into())]);
        let json = serde_json::json!({
            "arguments": {
                "game": ["--username", "${auth_player_name}"]
            }
        });
        assert_eq!(game_arguments(&json, &variables), ["--username", "Alex"]);
    }

    #[test]
    fn expands_legacy_game_arguments() {
        let variables = HashMap::from([("auth_player_name", "Alex".into())]);
        let json = serde_json::json!({
            "minecraftArguments": "--username ${auth_player_name} --demo"
        });
        assert_eq!(game_arguments(&json, &variables), ["--username", "Alex", "--demo"]);
    }

    #[test]
    fn selects_stable_loader_versions_in_numeric_order() {
        assert!(compare_loader_versions("21.1.264", "21.1.100").is_gt());
        assert!(compare_loader_versions("21.1.264", "21.1.264-beta").is_gt());
        assert_eq!(neoforge_line("1.21.1"), Some("21.1".into()));
        assert_eq!(neoforge_line("1.20.4"), Some("20.4".into()));
        assert_eq!(neoforge_line("snapshot"), None);
    }

    #[test]
    fn composes_loader_profile_over_mojang_metadata() {
        let base = serde_json::json!({
            "id": "1.21.1",
            "mainClass": "net.minecraft.client.main.Main",
            "libraries": [{"name": "com.example:base:1.0"}],
            "arguments": {"game": ["--username", "${auth_player_name}"]}
        });
        let profile = serde_json::json!({
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "libraries": [
                {"name": "com.example:loader:2.0"},
                {"name": "com.example:base:1.0"}
            ],
            "arguments": {
                "game": ["--launchTarget", "forgeclient"],
                "jvm": ["-DlibraryDirectory=${library_directory}"]
            }
        });
        let merged = compose_installer_profile(base, &profile, "1.21.1", "forge-profile").unwrap();
        assert_eq!(merged["id"], "forge-profile");
        assert_eq!(merged["inheritsFrom"], "1.21.1");
        assert_eq!(merged["mainClass"], "cpw.mods.bootstraplauncher.BootstrapLauncher");
        assert_eq!(merged["libraries"].as_array().unwrap().len(), 2);
        assert_eq!(merged["arguments"]["game"].as_array().unwrap().len(), 4);
        assert_eq!(merged["arguments"]["jvm"][0], "-DlibraryDirectory=${library_directory}");
    }
}
