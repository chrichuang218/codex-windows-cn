#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_windows_cn::{
    bridge::{
        self, AppStatus, InstallEvent, InstallRequest, InstallStart, InstallerDefaults,
        LauncherUpdateAction, LauncherUpdateActionResult, LauncherUpdateStart,
        LauncherUpdateStatus, ProxyLaunchResult, ProxyLaunchStatus, UninstallConfirmation,
        UninstallEvent, UninstallStart, UninstallStatus, UpdateAction, UpdateActionResult,
        UpdateStart, UpdateStatus,
    },
    config::Config,
    installer, launcher_update, mode, proxy, uninstall, updater,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;
use tauri::{Emitter, Manager};
#[cfg(windows)]
use windows::core::w;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

static INSTALL_EVENT: OnceLock<Mutex<Option<InstallEvent>>> = OnceLock::new();
static INSTALL_CANCEL: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
static UPDATE_EVENT: OnceLock<Mutex<Option<bridge::UpdateEvent>>> = OnceLock::new();
static LAUNCHER_UPDATE_EVENT: OnceLock<Mutex<Option<bridge::LauncherUpdateEvent>>> =
    OnceLock::new();
static UNINSTALL_EVENT: OnceLock<Mutex<Option<UninstallEvent>>> = OnceLock::new();

#[cfg(windows)]
struct SingleInstanceGuard(HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn install_event_state() -> &'static Mutex<Option<InstallEvent>> {
    INSTALL_EVENT.get_or_init(|| Mutex::new(None))
}

fn install_cancel_state() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    INSTALL_CANCEL.get_or_init(|| Mutex::new(None))
}

fn update_event_state() -> &'static Mutex<Option<bridge::UpdateEvent>> {
    UPDATE_EVENT.get_or_init(|| Mutex::new(None))
}

fn launcher_update_event_state() -> &'static Mutex<Option<bridge::LauncherUpdateEvent>> {
    LAUNCHER_UPDATE_EVENT.get_or_init(|| Mutex::new(None))
}

fn uninstall_event_state() -> &'static Mutex<Option<UninstallEvent>> {
    UNINSTALL_EVENT.get_or_init(|| Mutex::new(None))
}

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

    if let Ok(mut current) = install_event_state().lock() {
        *current = None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut current) = install_cancel_state().lock() {
        *current = Some(cancel.clone());
    }

    std::thread::spawn(move || {
        installer::run_cancellable(options, cancel, move |msg| {
            let event = bridge::install_event_from_msg(msg);
            if let Ok(mut current) = install_event_state().lock() {
                *current = Some(event.clone());
            }
            let _ = app.emit("install://event", event);
        });
        if let Ok(mut current) = install_cancel_state().lock() {
            *current = None;
        }
    });

    Ok(InstallStart { accepted: true })
}

#[tauri::command]
fn cancel_install() -> Result<InstallStart, String> {
    let Some(cancel) = install_cancel_state()
        .lock()
        .ok()
        .and_then(|current| current.clone())
    else {
        return Err("当前没有正在运行的安装任务".into());
    };
    cancel.store(true, Ordering::SeqCst);
    Ok(InstallStart { accepted: true })
}

#[tauri::command]
fn install_status() -> Option<InstallEvent> {
    install_event_state()
        .lock()
        .ok()
        .and_then(|current| current.clone())
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

    if let Ok(mut current) = update_event_state().lock() {
        *current = None;
    }

    std::thread::spawn(move || {
        installer::update(root, move |msg| {
            let event = bridge::update_event_from_msg(msg);
            if let Ok(mut current) = update_event_state().lock() {
                *current = Some(event.clone());
            }
            let _ = app.emit("update://event", event);
        });
    });

    Ok(UpdateStart { accepted: true })
}

#[tauri::command]
fn update_status() -> Option<bridge::UpdateEvent> {
    update_event_state()
        .lock()
        .ok()
        .and_then(|current| current.clone())
}

#[tauri::command]
fn check_launcher_update_status() -> Result<LauncherUpdateStatus, String> {
    let Ok((root, mut cfg)) = proxy_context() else {
        return Ok(bridge::launcher_update_status_from_decision(
            updater::LauncherDecision::Skipped {
                reason: "尚未完成安装".into(),
            },
        ));
    };

    if let Some(decision) = updater::pending_launcher_from_state(&cfg) {
        return Ok(bridge::launcher_update_status_from_decision(decision));
    }

    let will_query = updater::launcher_auto_check_will_query(&cfg);
    let decision = updater::check_launcher_auto(&cfg);
    if will_query {
        updater::record_launcher_check(&mut cfg, &decision);
        let _ = cfg.save_runtime(&root);
    }

    Ok(bridge::launcher_update_status_from_decision(decision))
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

    if let Ok(mut current) = launcher_update_event_state().lock() {
        *current = None;
    }

    std::thread::spawn(move || {
        launcher_update::apply(&latest_version, move |msg| {
            let event = bridge::launcher_update_event_from_msg(msg);
            if let Ok(mut current) = launcher_update_event_state().lock() {
                *current = Some(event.clone());
            }
            let _ = app.emit("launcher-update://event", event);
        });
    });

    Ok(LauncherUpdateStart { accepted: true })
}

#[tauri::command]
fn launcher_update_progress() -> Option<bridge::LauncherUpdateEvent> {
    launcher_update_event_state()
        .lock()
        .ok()
        .and_then(|current| current.clone())
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

    if let Ok(mut current) = uninstall_event_state().lock() {
        *current = None;
    }

    std::thread::spawn(move || {
        uninstall::run_worker(ctx, move |msg| {
            let event = bridge::uninstall_event_from_msg(msg);
            if let Ok(mut current) = uninstall_event_state().lock() {
                *current = Some(event.clone());
            }
            let _ = app.emit("uninstall://event", event);
        });
    });

    Ok(UninstallStart { accepted: true })
}

#[tauri::command]
fn uninstall_progress() -> Option<UninstallEvent> {
    uninstall_event_state()
        .lock()
        .ok()
        .and_then(|current| current.clone())
}

fn proxy_context() -> Result<(std::path::PathBuf, Config), String> {
    let root = mode::install_root().map_err(|cause| format!("无法读取安装目录：{cause:#}"))?;
    let cfg = Config::load_runtime(&root).map_err(|cause| format!("尚未完成安装：{cause:#}"))?;
    Ok((root, cfg))
}

fn install_main_window_show_fallback(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

#[cfg(windows)]
fn claim_single_instance_or_focus_existing() -> Option<SingleInstanceGuard> {
    let mutex = unsafe {
        CreateMutexW(
            None,
            true,
            w!("Local\\io.github.chrichuang218.codex-windows-cn.launcher"),
        )
    };
    let Ok(mutex) = mutex else {
        return Some(SingleInstanceGuard(HANDLE::default()));
    };

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            if let Ok(hwnd) = FindWindowW(None, w!("Codex Windows 中文助手")) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
            let _ = CloseHandle(mutex);
        }
        return None;
    }

    Some(SingleInstanceGuard(mutex))
}

#[cfg(not(windows))]
fn claim_single_instance_or_focus_existing() -> Option<()> {
    Some(())
}

fn main() {
    if std::env::args().any(|arg| arg == "--self-test") {
        return;
    }

    let Some(_single_instance_guard) = claim_single_instance_or_focus_existing() else {
        return;
    };

    tauri::Builder::default()
        .setup(|app| {
            install_main_window_show_fallback(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            installer_defaults,
            start_install,
            cancel_install,
            install_status,
            proxy_launch_status,
            launch_codex,
            check_update_status,
            apply_update_action,
            start_update,
            update_status,
            check_launcher_update_status,
            apply_launcher_update_action,
            start_launcher_update,
            launcher_update_progress,
            uninstall_confirmation,
            uninstall_status,
            uninstall_progress,
            start_uninstall
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Codex Windows 中文助手");
}
