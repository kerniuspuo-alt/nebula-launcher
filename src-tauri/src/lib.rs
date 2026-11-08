mod auth;
mod instances;
mod jvm;
mod launch;
mod minecraft;
mod overlay;
mod packs;
mod paths;
mod playtime;
mod scanner;
mod servers;
mod spotify;
mod system;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            paths::initialize(app.handle())?;
            if let Err(error) = overlay::start(app.handle().clone()) {
                eprintln!("Could not start companion telemetry listener: {error}");
            }
            if let Err(error) = spotify::start(app.handle().clone()) {
                eprintln!("Could not start Spotify callback listener: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system::get_system_info,
            jvm::list_java_runtimes,
            jvm::plan_jvm,
            jvm::adoptium_download_hint,
            jvm::download_temurin,
            instances::list_instances,
            instances::upsert_instance,
            instances::delete_instance,
            instances::touch_instance,
            instances::instance_dir,
            instances::list_worlds,
            minecraft::fetch_version_manifest,
            minecraft::fetch_version_json,
            minecraft::performance_profiles,
            minecraft::search_modrinth,
            minecraft::modrinth_versions,
            minecraft::search_curseforge_mods,
            minecraft::curseforge_mod_files,
            minecraft::install_curseforge_file,
            minecraft::download_modrinth_version,
            servers::ping_server,
            servers::curated_servers,
            auth::ms_device_code_start,
            auth::ms_device_code_poll,
            auth::list_accounts,
            auth::active_account,
            auth::set_active_account,
            auth::remove_account,
            auth::set_pin,
            auth::clear_pin,
            auth::change_pin,
            auth::verify_pin,
            auth::has_pin,
            scanner::scan_jar,
            scanner::scan_instance_mods,
            packs::mix_resource_packs,
            packs::generate_schematic,
            playtime::list_sessions,
            playtime::log_session,
            playtime::playtime_summary,
            launch::install_version,
            launch::install_instance,
            launch::launch_instance,
            launch::save_crosshair_png,
            spotify::spotify_authorize_url,
            spotify::spotify_access_token,
            spotify::spotify_current_user,
            spotify::spotify_disconnect,
        ])
        .run(tauri::generate_context!())
        .expect("error while running nebula");
}
