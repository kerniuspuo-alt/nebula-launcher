use serde::Serialize;
use sysinfo::System;

#[derive(Serialize, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub total_ram_mb: u64,
    pub used_ram_mb: u64,
    pub cpu_brand: String,
    pub cpu_cores: usize,
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    let mut sys = System::new_all();
    sys.refresh_all();
    let total = sys.total_memory() / 1024 / 1024;
    let used = sys.used_memory() / 1024 / 1024;
    let cpu = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_else(|| "Unknown CPU".into());
    SystemInfo {
        os: format!("{} {}", std::env::consts::OS, System::os_version().unwrap_or_default()),
        arch: std::env::consts::ARCH.to_string(),
        hostname: System::host_name().unwrap_or_else(|| "unknown".into()),
        total_ram_mb: total,
        used_ram_mb: used,
        cpu_brand: cpu,
        cpu_cores: sys.cpus().len(),
    }
}

/// Client heap: ~half of RAM, clamped so the OS and launcher stay responsive.
pub fn recommended_heap_mb(total_ram_mb: u64) -> u64 {
    let half = total_ram_mb / 2;
    half.clamp(2048, 8192).min(total_ram_mb.saturating_sub(2048).max(1024))
}
