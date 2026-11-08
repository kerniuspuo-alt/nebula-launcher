use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::paths;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub instance_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub seconds: u64,
    pub kind: String,
}

#[derive(Serialize, Deserialize, Default)]
struct LogFile {
    sessions: Vec<Session>,
}

fn path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(paths::app_dir(app)?.join("playtime.json"))
}

#[tauri::command]
pub fn list_sessions(app: AppHandle) -> Result<Vec<Session>, String> {
    let p = path(&app)?;
    if !p.exists() {
        return Ok(vec![]);
    }
    Ok(paths::read_json::<LogFile>(&p)?.sessions)
}

#[tauri::command]
pub fn log_session(app: AppHandle, session: Session) -> Result<(), String> {
    let p = path(&app)?;
    let mut data = if p.exists() {
        paths::read_json::<LogFile>(&p)?
    } else {
        LogFile::default()
    };
    data.sessions.push(session);
    paths::write_json(&p, &data)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeSummary {
    pub lifetime_seconds: u64,
    pub week_seconds: u64,
    pub sessions: usize,
}

#[tauri::command]
pub fn playtime_summary(app: AppHandle) -> Result<PlaytimeSummary, String> {
    let sessions = list_sessions(app)?;
    let week_cutoff = (Utc::now() - chrono::Duration::days(7)).timestamp();
    let week: u64 = sessions
        .iter()
        .filter(|s| {
            chrono::DateTime::parse_from_rfc3339(&s.started_at)
                .map(|d| d.timestamp() >= week_cutoff)
                .unwrap_or(false)
        })
        .map(|s| s.seconds)
        .sum();
    Ok(PlaytimeSummary {
        lifetime_seconds: sessions.iter().map(|s| s.seconds).sum(),
        week_seconds: week,
        sessions: sessions.len(),
    })
}
