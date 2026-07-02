#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_windows_cn::bridge::{self, AppStatus};

#[tauri::command]
fn app_status() -> AppStatus {
    bridge::app_status()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_status])
        .run(tauri::generate_context!())
        .expect("failed to run Codex Windows 中文助手");
}
