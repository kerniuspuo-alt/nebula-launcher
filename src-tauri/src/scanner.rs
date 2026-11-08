use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use tauri::AppHandle;

#[derive(Deserialize, Serialize, Clone)]
struct CachedFinding {
    sha256: String,
    blocked: bool,
    reasons: Vec<String>,
}

const SUSPICIOUS_NEEDLES: &[&str] = &[
    "discord.com/api/webhooks",
    "discordapp.com/api/webhooks",
    "api.ipify.org",
    "webhook.pw",
    "stealer",
    "sessionid",
    "level.dat_old",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinding {
    pub path: String,
    pub sha256: String,
    pub blocked: bool,
    pub reasons: Vec<String>,
}

#[tauri::command]
pub fn scan_jar(path: String) -> Result<ScanFinding, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let sha = hex::encode(Sha256::digest(&bytes));
    scan_bytes(path, bytes, sha)
}

fn scan_bytes(path: String, bytes: Vec<u8>, sha: String) -> Result<ScanFinding, String> {
    let mut reasons = Vec::new();

    if let Ok(mut file) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
        for i in 0..file.len() {
            if let Ok(mut entry) = file.by_index(i) {
                let mut buf = Vec::new();
                if entry.size() < 2_000_000 {
                    let _ = entry.read_to_end(&mut buf);
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        let lower = text.to_ascii_lowercase();
                        for needle in SUSPICIOUS_NEEDLES {
                            if lower.contains(needle) {
                                reasons.push(format!("Matched `{needle}` in {}", entry.name()));
                            }
                        }
                    }
                }
                let name = entry.name().to_ascii_lowercase();
                if name.ends_with(".class") && name.contains("discord") && name.contains("webhook") {
                    reasons.push(format!("Suspicious class {}", entry.name()));
                }
            }
        }
    }

    Ok(ScanFinding {
        path,
        sha256: sha,
        blocked: !reasons.is_empty(),
        reasons,
    })
}

#[tauri::command]
pub fn scan_instance_mods(app: AppHandle, instance_id: String) -> Result<Vec<ScanFinding>, String> {
    let dir = crate::paths::instances_dir(&app)?.join(instance_id).join("mods");
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let cache_path = crate::paths::app_dir(&app)?.join("jar-scan-cache.json");
    let mut cache: std::collections::HashMap<String, CachedFinding> = if cache_path.exists() {
        crate::paths::read_json(&cache_path)?
    } else {
        std::collections::HashMap::new()
    };
    let mut current_paths = std::collections::HashSet::new();
    for entry in walkdir::WalkDir::new(dir).max_depth(2) {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("jar"))
        {
            let path = entry.path().to_string_lossy().to_string();
            current_paths.insert(path.clone());
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let sha256 = hex::encode(Sha256::digest(&bytes));
            if let Some(cached) = cache.get(&path).filter(|cached| cached.sha256 == sha256) {
                out.push(ScanFinding {
                    path,
                    sha256: cached.sha256.clone(),
                    blocked: cached.blocked,
                    reasons: cached.reasons.clone(),
                });
                continue;
            }
            let finding = scan_bytes(path.clone(), bytes, sha256.clone())?;
            cache.insert(path, CachedFinding {
                sha256,
                blocked: finding.blocked,
                reasons: finding.reasons.clone(),
            });
            out.push(finding);
        }
    }
    cache.retain(|path, _| current_paths.contains(path));
    crate::paths::write_json(&cache_path, &cache)?;
    Ok(out)
}
