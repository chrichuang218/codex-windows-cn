#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_windows_cn::{
    bridge::{
        self, AppStatus, InstallRequest, InstallStart, InstallerDefaults, LauncherUpdateAction,
        LauncherUpdateActionResult, LauncherUpdateStart, LauncherUpdateStatus, ProxyLaunchResult,
        ProxyLaunchStatus, UninstallConfirmation, UninstallStart, UninstallStatus, UpdateAction,
        UpdateActionResult, UpdateStart, UpdateStatus,
    },
    config::Config,
    installer, launcher_update, mode, proxy, uninstall, updater,
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

#[tauri::command]
fn proxy_launch_status() -> Result<ProxyLaunchStatus, String> {
    let (root, cfg) = proxy_context()?;
    Ok(bridge::proxy_launch_status(&root, &cfg))
}

#[tauri::command]
fn launch_codex() -> Result<ProxyLaunchResult, String> {
    let (root, cfg) = proxy_context()?;
    proxy::launch(&root, &cfg, &[]).map_err(|cause| format!("启动 Codex 失败：{cause:#}"))?;
    Ok(ProxyLaunchResult {
        launched: true,
        message: "已启动 Codex".into(),
    })
}

#[tauri::command]
fn check_update_status() -> Result<UpdateStatus, String> {
    let (_root, cfg) = proxy_context()?;
    Ok(bridge::check_update_status(&cfg))
}

#[tauri::command]
fn apply_update_action(
    action: UpdateAction,
    latest_version: String,
) -> Result<UpdateActionResult, String> {
    let (root, mut cfg) = proxy_context()?;
    bridge::apply_update_action(&mut cfg, action, &latest_version);
    cfg.save_runtime(&root)
        .map_err(|cause| format!("保存更新设置失败：{cause:#}"))?;
    Ok(UpdateActionResult {
        applied: true,
        message: "已保存更新提醒设置".into(),
    })
}

#[tauri::command]
fn start_update(app: tauri::AppHandle) -> Result<UpdateStart, String> {
    let (root, _cfg) = proxy_context()?;

    std::thread::spawn(move || {
        installer::update(root, move |msg| {
            let _ = app.emit("update://event", bridge::update_event_from_msg(msg));
        });
    });

    Ok(UpdateStart { accepted: true })
}

#[tauri::command]
fn check_launcher_update_status() -> Result<LauncherUpdateStatus, String> {
    Ok(bridge::launcher_update_status_from_decision(
        updater::check_launcher_now(),
    ))
}

#[tauri::command]
fn apply_launcher_update_action(
    action: LauncherUpdateAction,
    latest_version: String,
) -> Result<LauncherUpdateActionResult, String> {
    let (root, mut cfg) = proxy_context()?;
    bridge::apply_launcher_update_action(&mut cfg, action, &latest_version);
    cfg.save_runtime(&root)
        .map_err(|cause| format!("保存自更新设置失败：{cause:#}"))?;
    Ok(LauncherUpdateActionResult {
        applied: true,
        message: "已保存自更新提醒设置".into(),
    })
}

#[tauri::command]
fn start_launcher_update(
    app: tauri::AppHandle,
    latest_version: String,
) -> Result<LauncherUpdateStart, String> {
    let latest_version = latest_version.trim().to_string();
    if latest_version.is_empty() {
        return Err("缺少启动器目标版本".into());
    }

    std::thread::spawn(move || {
        launcher_update::apply(&latest_version, move |msg| {
            let _ = app.emit(
                "launcher-update://event",
                bridge::launcher_update_event_from_msg(msg),
            );
        });
    });

    Ok(LauncherUpdateStart { accepted: true })
}

#[tauri::command]
fn uninstall_confirmation() -> Result<UninstallConfirmation, String> {
    let ctx =
        uninstall::load_context().map_err(|cause| format!("无法读取卸载上下文：{cause:#}"))?;
    Ok(bridge::uninstall_confirmation(&ctx.root))
}

#[tauri::command]
fn uninstall_status() -> Result<UninstallStatus, String> {
    let ctx =
        uninstall::load_context().map_err(|cause| format!("无法读取卸载上下文：{cause:#}"))?;
    Ok(bridge::uninstall_status_for_root(&ctx.root))
}

#[tauri::command]
fn start_uninstall(app: tauri::AppHandle) -> Result<UninstallStart, String> {
    let ctx =
        uninstall::load_context().map_err(|cause| format!("无法读取卸载上下文：{cause:#}"))?;
    if uninstall::need_elevation(&ctx) {
        return Err("卸载所有用户安装需要管理员权限".into());
    }

    std::thread::spawn(move || {
        uninstall::run_worker(ctx, move |msg| {
            let _ = app.emit("uninstall://event", bridge::uninstall_event_from_msg(msg));
        });
    });

    Ok(UninstallStart { accepted: true })
}

fn proxy_context() -> Result<(std::path::PathBuf, Config), String> {
    let root = mode::install_root().map_err(|cause| format!("无法读取安装目录：{cause:#}"))?;
    let cfg = Config::load_runtime(&root).map_err(|cause| format!("尚未完成安装：{cause:#}"))?;
    Ok((root, cfg))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_status,
            installer_defaults,
            start_install,
            proxy_launch_status,
            launch_codex,
            check_update_status,
            apply_update_action,
            start_update,
            check_launcher_update_status,
            apply_launcher_update_action,
            start_launcher_update,
            uninstall_confirmation,
            uninstall_status,
            start_uninstall
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Codex Windows 中文助手");
}
