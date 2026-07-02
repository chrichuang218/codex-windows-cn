#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_windows_cn::{
    bridge::{self, AppStatus, InstallRequest, InstallStart, InstallerDefaults},
    installer,
};
use tauri::Emitter;

#[tauri::command]
fn app_status() -> AppStatus {
    bridge::app_status()
}

#[tauri::command]
fn installer_defaults() -> InstallerDefaults {
    bridge::installer_defaults()
}

#[tauri::command]
fn start_install(app: tauri::AppHandle, request: InstallRequest) -> Result<InstallStart, String> {
    let options = bridge::install_options_from_request(request)?;

    std::thread::spawn(move || {
        installer::run(options, move |msg| {
            let _ = app.emit("install://event", bridge::install_event_from_msg(msg));
        });
    });

    Ok(InstallStart { accepted: true })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_status,
            installer_defaults,
            start_install
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Codex Windows 中文助手");
}
