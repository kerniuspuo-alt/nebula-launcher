use futures_util::StreamExt;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri::State;
use tokio::process::Command;

use crate::paths;
use crate::system::recommended_heap_mb;

#[derive(Serialize, Clone)]
pub struct JavaRuntime {
    pub path: String,
    pub version: String,
    pub major: u32,
    pub vendor: String,
}

#[derive(Serialize, Clone)]
pub struct JvmPlan {
    pub required_major: u32,
    pub heap_mb: u64,
    pub flags: Vec<String>,
    pub rationale: String,
    pub matched: Option<JavaRuntime>,
}

pub fn parse_java_major(version_output: &str) -> u32 {
    // Handles both `1.8.0_412` and `21.0.4`
    let line = version_output.lines().next().unwrap_or(version_output);
    let digits: String = line
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.starts_with("1.") {
        digits
            .split('.')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(8)
    } else {
        digits
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
}

async fn probe_java(bin: &Path) -> Option<JavaRuntime> {
    let output = Command::new(bin).arg("-version").output().await.ok()?;
    let text = {
        let mut s = String::from_utf8_lossy(&output.stderr).to_string();
        if s.trim().is_empty() {
            s = String::from_utf8_lossy(&output.stdout).to_string();
        }
        s
    };
    let major = parse_java_major(&text);
    if major == 0 {
        return None;
    }
    let vendor = if text.to_lowercase().contains("temurin") {
        "Temurin"
    } else if text.to_lowercase().contains("openjdk") {
        "OpenJDK"
    } else if text.to_lowercase().contains("graal") {
        "GraalVM"
    } else {
        "Java"
    };
    Some(JavaRuntime {
        path: bin.to_string_lossy().to_string(),
        version: text.lines().next().unwrap_or("unknown").trim().to_string(),
        major,
        vendor: vendor.into(),
    })
}

fn common_java_bins(app: &AppHandle) -> Vec<PathBuf> {
    let mut bins = Vec::new();
    if let Ok(dir) = paths::runtimes_dir(app) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let home = entry.path();
                for rel in ["bin/java", "Contents/Home/bin/java"] {
                    let p = home.join(rel);
                    if p.exists() {
                        bins.push(p);
                    }
                }
                // Temurin unpacked as jdk-21+.../bin/java
                if let Ok(inner) = std::fs::read_dir(&home) {
                    for child in inner.flatten() {
                        let p = child.path().join("bin/java");
                        if p.exists() {
                            bins.push(p);
                        }
                        let mac = child.path().join("Contents/Home/bin/java");
                        if mac.exists() {
                            bins.push(mac);
                        }
                    }
                }
            }
        }
    }
    if let Ok(home) = std::env::var("JAVA_HOME") {
        bins.push(PathBuf::from(home).join("bin/java"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir("/Library/Java/JavaVirtualMachines") {
            for entry in entries.flatten() {
                bins.push(entry.path().join("Contents/Home/bin/java"));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for root in [
            r"C:\Program Files\Eclipse Adoptium",
            r"C:\Program Files\Java",
            r"C:\Program Files\Microsoft",
        ] {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    bins.push(entry.path().join(r"bin\java.exe"));
                }
            }
        }
    }
    bins.push(PathBuf::from("java"));
    bins
}

#[tauri::command]
pub async fn list_java_runtimes(app: AppHandle) -> Vec<JavaRuntime> {
    let mut seen_paths = std::collections::HashSet::new();
    let bins = common_java_bins(&app)
        .into_iter()
        .filter(|path| seen_paths.insert(path.clone()))
        .collect::<Vec<_>>();
    let mut found = futures_util::stream::iter(
        bins.into_iter()
            .map(|bin| async move { probe_java(&bin).await }),
    )
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    found.sort_by(|a, b| b.major.cmp(&a.major));
    found
}

pub fn required_java_major(mc_version: &str) -> u32 {
    let n = parse_mc_tuple(mc_version);
    // 1.20.5+ → 21, 1.18–1.20.4 → 17, 1.17 → 16, older → 8
    if n >= (1, 20, 5) {
        21
    } else if n >= (1, 18, 0) {
        17
    } else if n >= (1, 17, 0) {
        16
    } else {
        8
    }
}

fn parse_mc_tuple(v: &str) -> (u32, u32, u32) {
    let cleaned = v.trim().trim_start_matches('v');
    if cleaned.contains('w') || cleaned.contains("snapshot") || cleaned.contains("pre") {
        return (1, 21, 0); // snapshots track modern Java
    }
    let parts: Vec<u32> = cleaned
        .split('.')
        .filter_map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect();
    (
        parts.first().copied().unwrap_or(1),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

pub fn aikar_flags(heap_mb: u64, java_major: u32) -> Vec<String> {
    let mut flags = vec![
        format!("-Xms{heap_mb}M"),
        format!("-Xmx{heap_mb}M"),
        "-XX:+UnlockExperimentalVMOptions".into(),
        "-XX:+DisableExplicitGC".into(),
        "-XX:+AlwaysPreTouch".into(),
        "-XX:SurvivorRatio=32".into(),
        "-XX:+PerfDisableSharedMem".into(),
        "-Dfile.encoding=UTF-8".into(),
        "-Dstdout.encoding=UTF-8".into(),
        "-Dstderr.encoding=UTF-8".into(),
    ];
    if java_major >= 21 {
        flags.extend(["-XX:+UseZGC".into(), "-XX:+ZGenerational".into()]);
    } else {
        flags.extend([
            "-XX:+UseG1GC".into(),
            "-XX:+ParallelRefProcEnabled".into(),
            "-XX:MaxGCPauseMillis=200".into(),
            "-XX:G1NewSizePercent=30".into(),
            "-XX:G1MaxNewSizePercent=40".into(),
            "-XX:G1HeapRegionSize=8M".into(),
            "-XX:G1ReservePercent=20".into(),
            "-XX:G1HeapWastePercent=5".into(),
            "-XX:InitiatingHeapOccupancyPercent=15".into(),
            "-Dusing.aikars.flags=https://mcflags.emc.gs".into(),
            "-Daikars.new.flags=true".into(),
        ]);
    }
    flags
}

#[tauri::command]
pub async fn plan_jvm(
    app: AppHandle,
    minecraft_version: String,
    total_ram_mb: Option<u64>,
) -> JvmPlan {
    let required = required_java_major(&minecraft_version);
    let ram = total_ram_mb.unwrap_or_else(|| crate::system::get_system_info().total_ram_mb);
    let heap = recommended_heap_mb(ram);
    let matched = list_java_runtimes(app)
        .await
        .into_iter()
        .find(|rt| rt.major >= required);
    let rationale = format!(
        "Minecraft {minecraft_version} needs Java {required}+. Heap {heap} MB from {ram} MB RAM. {}",
        if required >= 21 {
            "ZGC generational for Java 21+."
        } else {
            "Aikar G1 flags for Java 8–17."
        }
    );
    JvmPlan {
        required_major: required,
        heap_mb: heap,
        flags: aikar_flags(heap, required),
        rationale,
        matched,
    }
}

#[derive(Serialize)]
pub struct AdoptiumHint {
    pub feature: u32,
    pub os: String,
    pub arch: String,
    pub url: String,
}

#[tauri::command]
pub fn adoptium_download_hint() -> AdoptiumHint {
    let os = match std::env::consts::OS {
        "macos" => "mac",
        "windows" => "windows",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" | "x86" => "x64",
        "aarch64" => "aarch64",
        other => other,
    };
    let feature = 21u32;
    AdoptiumHint {
        url: format!(
            "https://api.adoptium.net/v3/binary/latest/{feature}/ga/{os}/{arch}/jdk/hotspot/normal/eclipse?project=jdk"
        ),
        feature,
        os: os.into(),
        arch: arch.into(),
    }
}

#[tauri::command]
pub async fn download_temurin(
    app: AppHandle,
    registry: State<'_, crate::launch::DownloadRegistry>,
    feature: u32,
    download_id: String,
) -> Result<String, String> {
    let session = registry.start(download_id)?;
    let result = download_temurin_inner(&app, feature, &session).await;
    if let Err(error) = &result {
        crate::launch::emit_download_failure(&app, &session, "Temurin Java runtime", error);
    }
    registry.finish(&session);
    result
}

async fn download_temurin_inner(
    app: &AppHandle,
    feature: u32,
    session: &crate::launch::DownloadContext,
) -> Result<String, String> {
    if !(8..=99).contains(&feature) {
        return Err("Choose a supported Java feature version".into());
    }
    let hint = adoptium_download_hint();
    let os = hint.os;
    let arch = hint.arch;
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{feature}/ga/{os}/{arch}/jdk/hotspot/normal/eclipse?project=jdk"
    );
    let dest_dir = paths::runtimes_dir(app)?.join(format!("jdk-{feature}"));
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|error| error.to_string())?;
    let archive_path = dest_dir.join("jdk-download.bin");
    crate::launch::download_file_with_client(
        app,
        session,
        crate::minecraft::shared_api_client(),
        &url,
        &archive_path,
    )
    .await?;
    extract_archive(&archive_path, &dest_dir).await?;
    let runtime = list_java_runtimes(app.clone())
        .await
        .into_iter()
        .find(|runtime| runtime.major == feature && Path::new(&runtime.path).starts_with(&dest_dir))
        .ok_or_else(|| {
            format!("Temurin Java {feature} downloaded but no usable runtime was found")
        })?;
    tokio::fs::remove_file(&archive_path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(runtime.path)
}

async fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let name = archive.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let mut magic = [0; 2];
    std::fs::File::open(archive)
        .and_then(|mut file| file.read_exact(&mut magic))
        .map_err(|error| error.to_string())?;
    if magic == *b"PK" {
        let archive = archive.to_path_buf();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(archive).map_err(|error| error.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
            zip.extract(dest).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())??;
        return Ok(());
    }
    let status = Command::new("tar")
        .args(["-xf"])
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .await
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "Could not extract Java archive ({name}). Install a JDK manually."
        ));
    }
    Ok(())
}
