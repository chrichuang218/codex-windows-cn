#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codex_windows_cn::{
    bridge::{
        self, AppStatus, BridgeInstallMode, CodexProtocolActionResult, DesktopShortcutActionResult,
        InstallEvent, InstallRequest, InstallStart, InstallerDefaults, LaunchInstalledRequest,
        LaunchRequest, LauncherUpdateAction, LauncherUpdateActionResult, LauncherUpdateStart,
        LauncherUpdateStatus, ProxyLaunchResult, ProxyLaunchStatus, UninstallConfirmation,
        UninstallEvent, UninstallStart, UninstallStatus, UpdateAction, UpdateActionResult,
        UpdateStart, UpdateStatus, VersionActionResult, VersionInventory, VersionSettingsRequest,
    },
    config::{Config, InstallMode},
    dialogs, elevate, installer, launcher_update, mode, proxy, uninstall, updater, versions,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
static RUNTIME_CONFIG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

fn runtime_config_lock() -> &'static Mutex<()> {
    RUNTIME_CONFIG_LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_runtime_config() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    runtime_config_lock()
        .lock()
        .map_err(|_| "配置状态锁已损坏，请重启中文助手".to_string())
}

fn publish_install_event(app: &tauri::AppHandle, event: InstallEvent) {
    if let Ok(mut current) = install_event_state().lock() {
        *current = Some(event.clone());
    }
    let _ = app.emit("install://event", event);
}

fn publish_update_event(app: &tauri::AppHandle, event: bridge::UpdateEvent) {
    if let Ok(mut current) = update_event_state().lock() {
        *current = Some(event.clone());
    }
    let _ = app.emit("update://event", event);
}

fn publish_uninstall_event(app: &tauri::AppHandle, event: UninstallEvent) {
    if let Ok(mut current) = uninstall_event_state().lock() {
        *current = Some(event.clone());
    }
    let _ = app.emit("uninstall://event", event);
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
    let options = bridge::install_options_from_request(request.clone())?;
    let needs_elevation =
        matches!(request.mode, BridgeInstallMode::System) && !elevate::is_elevated();

    if let Ok(mut current) = install_event_state().lock() {
        *current = None;
    }

    if needs_elevation {
        let (request_path, result_path) = write_elevated_install_request(&request)?;
        if let Ok(mut current) = install_cancel_state().lock() {
            *current = None;
        }
        publish_install_event(
            &app,
            bridge::install_event_from_msg(installer::InstallMsg::Phase {
                phase: "Elevating".into(),
                detail: "请在 Windows 提示中允许管理员权限。".into(),
            }),
        );
        std::thread::spawn(move || {
            let result = run_elevated_task(
                &format!(
                    "--install-system {} {}",
                    quote_cli_path(&request_path),
                    quote_cli_path(&result_path)
                ),
                &result_path,
                "系统安装",
            );
            let _ = std::fs::remove_file(&request_path);
            let _ = std::fs::remove_file(&result_path);
            let event = match result {
                Ok(version) => {
                    bridge::install_event_from_msg(installer::InstallMsg::Done { version })
                }
                Err(message) => {
                    bridge::install_event_from_msg(installer::InstallMsg::Error(message))
                }
            };
            publish_install_event(&app, event);
        });
        return Ok(InstallStart {
            accepted: true,
            cancellable: false,
        });
    }

    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut current) = install_cancel_state().lock() {
        *current = Some(cancel.clone());
    }

    std::thread::spawn(move || {
        installer::run_cancellable(options, cancel, move |msg| {
            let event = bridge::install_event_from_msg(msg);
            publish_install_event(&app, event);
        });
        if let Ok(mut current) = install_cancel_state().lock() {
            *current = None;
        }
    });

    Ok(InstallStart {
        accepted: true,
        cancellable: true,
    })
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
    Ok(InstallStart {
        accepted: true,
        cancellable: true,
    })
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
fn launch_codex(request: Option<LaunchRequest>) -> Result<ProxyLaunchResult, String> {
    let (root, cfg) = proxy_context()?;
    let request = request.unwrap_or(LaunchRequest {
        version: None,
        switch_running: false,
    });
    let outcome = proxy::launch_version(
        &root,
        cfg.use_current_junction,
        request.version.as_deref(),
        request.switch_running,
        &[],
    )
    .map_err(|cause| format!("启动应用失败：{cause:#}"))?;
    Ok(bridge::launch_result_from_outcome(outcome))
}

#[tauri::command]
fn launch_installed_codex(request: LaunchInstalledRequest) -> Result<ProxyLaunchResult, String> {
    let root = request.root.trim();
    if root.is_empty() {
        return Err("请选择安装位置".into());
    }

    let outcome = proxy::launch_version(
        std::path::Path::new(root),
        request.use_current_junction,
        None,
        false,
        &[],
    )
    .map_err(|cause| format!("启动应用失败：{cause:#}"))?;
    Ok(bridge::launch_result_from_outcome(outcome))
}

#[tauri::command]
fn get_version_inventory() -> Result<VersionInventory, String> {
    let (root, cfg) = proxy_context()?;
    bridge::version_inventory(&root, &cfg).map_err(|cause| format!("读取已安装版本失败：{cause:#}"))
}

#[tauri::command]
fn save_version_settings(request: VersionSettingsRequest) -> Result<VersionActionResult, String> {
    let _guard = lock_runtime_config()?;
    let (root, mut cfg) = proxy_context()?;
    bridge::persist_version_settings(&root, &mut cfg, request)
        .map_err(|cause| format!("保存版本策略失败：{cause:#}"))
}

#[tauri::command]
fn set_desktop_shortcut(enabled: bool) -> Result<DesktopShortcutActionResult, String> {
    let (root, cfg) = proxy_context()?;
    if matches!(cfg.install_mode, InstallMode::System) && !elevate::is_elevated() {
        let action = if enabled { "create" } else { "remove" };
        let exit_code = elevate::respawn_elevated_wait(&format!("--set-desktop-shortcut {action}"))
            .map_err(|_| "管理公共桌面快捷方式需要管理员权限，授权未完成".to_string())?;
        if exit_code != 0 {
            return Err("管理员快捷方式进程执行失败，未修改桌面入口".into());
        }
    } else {
        apply_desktop_shortcut(&root, &cfg, enabled)?;
    }

    let inventory = bridge::version_inventory(&root, &cfg)
        .map_err(|cause| format!("刷新桌面快捷方式状态失败：{cause:#}"))?;
    Ok(DesktopShortcutActionResult {
        applied: true,
        message: if enabled {
            "已创建 ChatGPT 桌面快捷方式"
        } else {
            "已移除 ChatGPT 桌面快捷方式"
        }
        .into(),
        inventory,
    })
}

fn apply_desktop_shortcut(
    root: &std::path::Path,
    cfg: &Config,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        installer::create_or_update_desktop_shortcut(
            root,
            cfg.install_mode,
            cfg.use_current_junction,
        )
        .map_err(|cause| format!("创建 ChatGPT 桌面快捷方式失败：{cause:#}"))
    } else {
        installer::remove_desktop_shortcut(root, cfg.install_mode)
            .map_err(|cause| format!("移除 ChatGPT 桌面快捷方式失败：{cause:#}"))
    }
}

#[tauri::command]
fn set_assistant_desktop_shortcut(enabled: bool) -> Result<DesktopShortcutActionResult, String> {
    let (root, cfg) = proxy_context()?;
    if matches!(cfg.install_mode, InstallMode::System) && !elevate::is_elevated() {
        let action = if enabled { "create" } else { "remove" };
        let exit_code = elevate::respawn_elevated_wait(&format!(
            "--set-assistant-desktop-shortcut {action}"
        ))
        .map_err(|_| "管理公共桌面的中文助手快捷方式需要管理员权限，授权未完成".to_string())?;
        if exit_code != 0 {
            return Err("管理员快捷方式进程执行失败，未修改中文助手桌面入口".into());
        }
    } else {
        apply_assistant_desktop_shortcut(&root, &cfg, enabled)?;
    }

    let inventory = bridge::version_inventory(&root, &cfg)
        .map_err(|cause| format!("刷新中文助手桌面快捷方式状态失败：{cause:#}"))?;
    Ok(DesktopShortcutActionResult {
        applied: true,
        message: if enabled {
            "已创建中文助手桌面快捷方式"
        } else {
            "已移除中文助手桌面快捷方式"
        }
        .into(),
        inventory,
    })
}

fn apply_assistant_desktop_shortcut(
    root: &std::path::Path,
    cfg: &Config,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        installer::create_or_update_assistant_desktop_shortcut(root, cfg.install_mode)
            .map_err(|cause| format!("创建中文助手桌面快捷方式失败：{cause:#}"))
    } else {
        installer::remove_assistant_desktop_shortcut(root, cfg.install_mode)
            .map_err(|cause| format!("移除中文助手桌面快捷方式失败：{cause:#}"))
    }
}

#[tauri::command]
fn set_codex_protocol(
    enabled: bool,
    replace_other: bool,
) -> Result<CodexProtocolActionResult, String> {
    let _guard = lock_runtime_config()?;
    let (root, mut cfg) = proxy_context()?;
    let message = if matches!(cfg.install_mode, InstallMode::System) && !elevate::is_elevated() {
        let action = if !enabled {
            "remove"
        } else if replace_other {
            "replace"
        } else {
            "create"
        };
        let exit_code = elevate::respawn_elevated_wait(&format!("--set-codex-protocol {action}"))
            .map_err(|_| {
            "管理系统级 codex:// 会话链接需要管理员权限，授权未完成".to_string()
        })?;
        if exit_code != 0 {
            return Err("管理员协议配置进程执行失败，未修改 codex:// 会话链接".into());
        }
        cfg.register_codex_protocol = Some(enabled);
        cfg.save_runtime(&root)
            .map_err(|cause| format!("保存 codex:// 会话链接偏好失败：{cause:#}"))?;
        protocol_action_message(enabled, replace_other).to_string()
    } else {
        let persist_install = matches!(cfg.install_mode, InstallMode::System);
        apply_codex_protocol_change(&root, &mut cfg, enabled, replace_other, persist_install)?
    };

    let inventory = bridge::version_inventory(&root, &cfg)
        .map_err(|cause| format!("刷新 codex:// 会话链接状态失败：{cause:#}"))?;
    Ok(CodexProtocolActionResult {
        applied: true,
        message,
        inventory,
    })
}

fn apply_codex_protocol_change(
    root: &Path,
    cfg: &mut Config,
    enabled: bool,
    replace_other: bool,
    persist_install: bool,
) -> Result<String, String> {
    if enabled {
        if matches!(
            installer::enable_codex_protocol(root, cfg, replace_other)
                .map_err(|cause| format!("配置 codex:// 会话链接失败：{cause:#}"))?,
            codex_windows_cn::registry::ProtocolRegistration::PreservedForeign
        ) {
            return Err(
                "codex:// 当前由其他 Codex 安装处理；如需切换，请使用“设为当前安装”".into(),
            );
        }
    } else {
        let _ = installer::disable_codex_protocol(root, cfg)
            .map_err(|cause| format!("移除 codex:// 会话链接失败：{cause:#}"))?;
    }

    cfg.register_codex_protocol = Some(enabled);
    if persist_install {
        cfg.save_install(root)
    } else {
        cfg.save_runtime(root)
    }
    .map_err(|cause| format!("保存 codex:// 会话链接偏好失败：{cause:#}"))?;
    Ok(protocol_action_message(enabled, replace_other).to_string())
}

fn protocol_action_message(enabled: bool, replace_other: bool) -> &'static str {
    if !enabled {
        "已停止由当前安装处理 codex:// 会话链接"
    } else if replace_other {
        "已将 codex:// 会话链接切换到当前安装"
    } else {
        "已创建或修复 codex:// 会话链接"
    }
}

#[tauri::command]
fn delete_installed_version(version: String) -> Result<VersionActionResult, String> {
    let _guard = lock_runtime_config()?;
    let (root, mut cfg) = proxy_context()?;
    if !versions::is_version_name(&version) {
        return Err("版本号格式无效".into());
    }
    if matches!(cfg.install_mode, InstallMode::System) && !elevate::is_elevated() {
        let exit_code =
            elevate::respawn_elevated_wait(&format!("--delete-installed-version {version}"))
                .map_err(|_| "删除系统版本需要管理员权限，授权未完成".to_string())?;
        if exit_code != 0 {
            return Err("管理员删除进程执行失败，版本未删除".into());
        }
        cfg = Config::load_runtime(&root)
            .map_err(|cause| format!("重新读取版本状态失败：{cause:#}"))?;
        let inventory = bridge::version_inventory(&root, &cfg)
            .map_err(|cause| format!("刷新已安装版本失败：{cause:#}"))?;
        return Ok(VersionActionResult {
            applied: true,
            message: format!("已删除版本 {version}"),
            inventory,
        });
    }

    delete_installed_version_inner(&root, &mut cfg, &version)
}

fn delete_installed_version_inner(
    root: &std::path::Path,
    cfg: &mut Config,
    version: &str,
) -> Result<VersionActionResult, String> {
    let running = proxy::running_versions(&root.join("versions"));
    let repair = versions::delete_and_repair(root, cfg, version, &running)
        .map_err(|cause| format!("删除版本失败：{cause:#}"))?;
    let protocol_warning = installer::sync_codex_protocol(root, cfg)
        .err()
        .map(|cause| format!("；codex:// 会话链接刷新失败：{cause:#}"))
        .unwrap_or_default();
    let inventory = bridge::version_inventory(root, cfg)
        .map_err(|cause| format!("刷新已安装版本失败：{cause:#}"))?;
    Ok(VersionActionResult {
        applied: true,
        message: if repair.current_repaired {
            format!("已删除版本 {version}{protocol_warning}")
        } else {
            format!(
                "已删除版本 {version}；current 入口将在下次启动时重试修复到 {}{}",
                repair.default_version, protocol_warning
            )
        },
        inventory,
    })
}

#[tauri::command]
fn check_update_status() -> Result<UpdateStatus, String> {
    let _guard = lock_runtime_config()?;
    let (root, mut cfg) = proxy_context()?;
    let product_name = versions::scan_installed(&root)
        .ok()
        .and_then(|installed| {
            installed
                .first()
                .map(|item| item.app_kind.display_name().to_string())
        })
        .unwrap_or_else(|| "Codex".into());
    let will_query = updater::auto_check_will_query(&cfg);
    let decision = if will_query {
        updater::check_auto(&cfg, codex_windows_cn::store::PRODUCT_ID_CODEX)
    } else {
        updater::cached_update_decision(&cfg, &product_name)
            .unwrap_or_else(|| updater::check_auto(&cfg, codex_windows_cn::store::PRODUCT_ID_CODEX))
    };
    if will_query {
        updater::record_auto_check(&mut cfg, &decision);
        cfg.save_runtime(&root)
            .map_err(|cause| format!("保存更新检查状态失败：{cause:#}"))?;
    }
    Ok(bridge::update_status_from_decision(decision))
}

#[tauri::command]
fn apply_update_action(
    action: UpdateAction,
    latest_version: String,
) -> Result<UpdateActionResult, String> {
    let _guard = lock_runtime_config()?;
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
    let (root, cfg) = proxy_context()?;

    if let Ok(mut current) = update_event_state().lock() {
        *current = None;
    }

    if matches!(cfg.install_mode, InstallMode::System) && !elevate::is_elevated() {
        let result_path = elevated_temp_path("update-result");
        publish_update_event(
            &app,
            bridge::update_event_from_msg(installer::InstallMsg::Phase {
                phase: "Elevating".into(),
                detail: "请在 Windows 提示中允许管理员权限。".into(),
            }),
        );
        std::thread::spawn(move || {
            let result = run_elevated_task(
                &format!("--update-system {}", quote_cli_path(&result_path)),
                &result_path,
                "系统更新",
            );
            let _ = std::fs::remove_file(&result_path);
            let event = match result {
                Ok(version) => {
                    bridge::update_event_from_msg(installer::InstallMsg::Done { version })
                }
                Err(message) => {
                    bridge::update_event_from_msg(installer::InstallMsg::Error(message))
                }
            };
            publish_update_event(&app, event);
        });
        return Ok(UpdateStart { accepted: true });
    }

    std::thread::spawn(move || {
        installer::update(root, move |msg| {
            let event = bridge::update_event_from_msg(msg);
            publish_update_event(&app, event);
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
    let _guard = lock_runtime_config()?;
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
        cfg.save_runtime(&root)
            .map_err(|cause| format!("保存启动器更新检查状态失败：{cause:#}"))?;
    }

    Ok(bridge::launcher_update_status_from_decision(decision))
}

#[tauri::command]
fn apply_launcher_update_action(
    action: LauncherUpdateAction,
    latest_version: String,
) -> Result<LauncherUpdateActionResult, String> {
    let _guard = lock_runtime_config()?;
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

    if let Ok(mut current) = uninstall_event_state().lock() {
        *current = None;
    }

    if uninstall::need_elevation(&ctx) {
        let result_path = elevated_temp_path("uninstall-result");
        publish_uninstall_event(
            &app,
            bridge::uninstall_event_from_msg(uninstall::UninstallMsg::Phase {
                phase: "Elevating".into(),
                detail: "请在 Windows 提示中允许管理员权限。".into(),
            }),
        );
        std::thread::spawn(move || {
            let result = run_elevated_task(
                &format!("--uninstall-system {}", quote_cli_path(&result_path)),
                &result_path,
                "系统卸载",
            );
            let _ = std::fs::remove_file(&result_path);
            let message = match result {
                Ok(log_path) => uninstall::UninstallMsg::Done { log_path },
                Err(message) => uninstall::UninstallMsg::Error(message),
            };
            publish_uninstall_event(&app, bridge::uninstall_event_from_msg(message));
        });
        return Ok(UninstallStart { accepted: true });
    }

    std::thread::spawn(move || {
        uninstall::run_worker(ctx, move |msg| {
            publish_uninstall_event(&app, bridge::uninstall_event_from_msg(msg));
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum ElevatedTaskResult {
    Success { value: String },
    Error { message: String },
}

fn elevated_temp_path(kind: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "codex-windows-cn-{kind}-{}-{nonce}.json",
        std::process::id()
    ))
}

fn write_elevated_install_request(request: &InstallRequest) -> Result<(PathBuf, PathBuf), String> {
    let request_path = elevated_temp_path("install-request");
    let result_path = elevated_temp_path("install-result");
    let raw =
        serde_json::to_vec(request).map_err(|cause| format!("准备系统安装请求失败：{cause}"))?;
    std::fs::write(&request_path, raw).map_err(|cause| format!("写入系统安装请求失败：{cause}"))?;
    Ok((request_path, result_path))
}

fn quote_cli_path(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn run_elevated_task(args: &str, result_path: &Path, operation: &str) -> Result<String, String> {
    let exit_code = elevate::respawn_elevated_wait(args)
        .map_err(|cause| format!("{operation}未获得管理员权限，操作未开始：{cause:#}"))?;
    let raw = std::fs::read(result_path)
        .map_err(|cause| format!("{operation}进程未返回结果（退出代码 {exit_code}）：{cause}"))?;
    let result: ElevatedTaskResult = serde_json::from_slice(&raw)
        .map_err(|cause| format!("{operation}结果无效（退出代码 {exit_code}）：{cause}"))?;
    match result {
        ElevatedTaskResult::Success { value } if exit_code == 0 => Ok(value),
        ElevatedTaskResult::Success { .. } => {
            Err(format!("{operation}进程异常退出（退出代码 {exit_code}）"))
        }
        ElevatedTaskResult::Error { message } => Err(message),
    }
}

fn write_elevated_task_result(path: &Path, result: &ElevatedTaskResult) -> Result<(), String> {
    let raw = serde_json::to_vec(result).map_err(|cause| format!("序列化执行结果失败：{cause}"))?;
    std::fs::write(path, raw).map_err(|cause| format!("写入执行结果失败：{cause}"))
}

fn finish_elevated_task(result_path: &Path, result: Result<String, String>) -> i32 {
    let succeeded = result.is_ok();
    let payload = match result {
        Ok(value) => ElevatedTaskResult::Success { value },
        Err(message) => ElevatedTaskResult::Error { message },
    };
    if let Err(message) = write_elevated_task_result(result_path, &payload) {
        dialogs::error(&format!(
            "管理员操作已结束，但无法把结果返回给中文助手：{message}"
        ));
        return 1;
    }
    if succeeded {
        0
    } else {
        1
    }
}

fn run_system_install_helper(request_path: &Path, result_path: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        if !elevate::is_elevated() {
            return Err("系统安装需要管理员权限".into());
        }
        let raw = std::fs::read(request_path)
            .map_err(|cause| format!("读取系统安装请求失败：{cause}"))?;
        let request: InstallRequest = serde_json::from_slice(&raw)
            .map_err(|cause| format!("解析系统安装请求失败：{cause}"))?;
        if !matches!(request.mode, BridgeInstallMode::System) {
            return Err("系统安装请求的安装范围无效".into());
        }
        let options = bridge::install_options_from_request(request)?;
        installer::run_elevated(options).map_err(|cause| format!("系统安装失败：{cause:#}"))
    })();
    finish_elevated_task(result_path, result)
}

fn run_system_update_helper(result_path: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        if !elevate::is_elevated() {
            return Err("系统更新需要管理员权限".into());
        }
        let (root, cfg) = proxy_context()?;
        if !matches!(cfg.install_mode, InstallMode::System) {
            return Err("当前安装不是所有用户安装，无法执行系统更新".into());
        }
        installer::update_elevated(root).map_err(|cause| format!("系统更新失败：{cause:#}"))
    })();
    finish_elevated_task(result_path, result)
}

fn run_system_uninstall_helper(result_path: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        if !elevate::is_elevated() {
            return Err("系统卸载需要管理员权限".into());
        }
        let ctx = uninstall::load_context()
            .map_err(|cause| format!("读取系统卸载上下文失败：{cause:#}"))?;
        if !matches!(ctx.cfg.install_mode, InstallMode::System) {
            return Err("当前安装不是所有用户安装，无法执行系统卸载".into());
        }

        let final_result = Mutex::new(None);
        uninstall::run_worker(ctx, |message| match message {
            uninstall::UninstallMsg::Done { log_path } => {
                if let Ok(mut current) = final_result.lock() {
                    *current = Some(Ok(log_path));
                }
            }
            uninstall::UninstallMsg::Error(message) => {
                if let Ok(mut current) = final_result.lock() {
                    *current = Some(Err(message));
                }
            }
            _ => {}
        });
        final_result
            .into_inner()
            .map_err(|_| "读取系统卸载结果失败".to_string())?
            .unwrap_or_else(|| Err("系统卸载进程未返回最终结果".into()))
    })();
    finish_elevated_task(result_path, result)
}

#[derive(Debug, PartialEq, Eq)]
enum CliHelperAction {
    Uninstall,
    DeleteInstalledVersion(String),
    SetDesktopShortcut(bool),
    SetAssistantDesktopShortcut(bool),
    SetCodexProtocol {
        enabled: bool,
        replace_other: bool,
    },
    InstallSystem {
        request_path: String,
        result_path: String,
    },
    UpdateSystem {
        result_path: String,
    },
    UninstallSystem {
        result_path: String,
    },
    LaunchLatest,
}

fn parse_cli_helper(mut args: impl Iterator<Item = String>) -> Result<Option<CliHelperAction>, ()> {
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    let action = match flag.as_str() {
        "--uninstall" => CliHelperAction::Uninstall,
        "--delete-installed-version" => {
            let version = args.next().ok_or(())?;
            CliHelperAction::DeleteInstalledVersion(version)
        }
        "--set-desktop-shortcut" => match args.next().as_deref() {
            Some("create") => CliHelperAction::SetDesktopShortcut(true),
            Some("remove") => CliHelperAction::SetDesktopShortcut(false),
            _ => return Err(()),
        },
        "--set-assistant-desktop-shortcut" => match args.next().as_deref() {
            Some("create") => CliHelperAction::SetAssistantDesktopShortcut(true),
            Some("remove") => CliHelperAction::SetAssistantDesktopShortcut(false),
            _ => return Err(()),
        },
        "--set-codex-protocol" => match args.next().as_deref() {
            Some("create") => CliHelperAction::SetCodexProtocol {
                enabled: true,
                replace_other: false,
            },
            Some("replace") => CliHelperAction::SetCodexProtocol {
                enabled: true,
                replace_other: true,
            },
            Some("remove") => CliHelperAction::SetCodexProtocol {
                enabled: false,
                replace_other: false,
            },
            _ => return Err(()),
        },
        "--install-system" => CliHelperAction::InstallSystem {
            request_path: args.next().ok_or(())?,
            result_path: args.next().ok_or(())?,
        },
        "--update-system" => CliHelperAction::UpdateSystem {
            result_path: args.next().ok_or(())?,
        },
        "--uninstall-system" => CliHelperAction::UninstallSystem {
            result_path: args.next().ok_or(())?,
        },
        "--launch-latest" => CliHelperAction::LaunchLatest,
        _ => return Ok(None),
    };
    if args.next().is_some() {
        return Err(());
    }
    Ok(Some(action))
}

fn run_cli_helper() -> Option<i32> {
    let action = match parse_cli_helper(std::env::args().skip(1)) {
        Ok(Some(action)) => action,
        Ok(None) => return None,
        Err(()) => return Some(2),
    };

    match action {
        CliHelperAction::Uninstall => Some(if uninstall::run().is_ok() { 0 } else { 1 }),
        CliHelperAction::DeleteInstalledVersion(version) => {
            let result = proxy_context().and_then(|(root, mut cfg)| {
                delete_installed_version_inner(&root, &mut cfg, &version).map(|_| ())
            });
            Some(if result.is_ok() { 0 } else { 1 })
        }
        CliHelperAction::SetDesktopShortcut(enabled) => {
            let result = proxy_context()
                .and_then(|(root, cfg)| apply_desktop_shortcut(&root, &cfg, enabled));
            Some(if result.is_ok() { 0 } else { 1 })
        }
        CliHelperAction::SetAssistantDesktopShortcut(enabled) => {
            let result = proxy_context()
                .and_then(|(root, cfg)| apply_assistant_desktop_shortcut(&root, &cfg, enabled));
            Some(if result.is_ok() { 0 } else { 1 })
        }
        CliHelperAction::SetCodexProtocol {
            enabled,
            replace_other,
        } => {
            let result = proxy_context().and_then(|(root, mut cfg)| {
                apply_codex_protocol_change(&root, &mut cfg, enabled, replace_other, true)
                    .map(|_| ())
            });
            Some(if result.is_ok() { 0 } else { 1 })
        }
        CliHelperAction::InstallSystem {
            request_path,
            result_path,
        } => Some(run_system_install_helper(
            Path::new(&request_path),
            Path::new(&result_path),
        )),
        CliHelperAction::UpdateSystem { result_path } => {
            Some(run_system_update_helper(Path::new(&result_path)))
        }
        CliHelperAction::UninstallSystem { result_path } => {
            Some(run_system_uninstall_helper(Path::new(&result_path)))
        }
        CliHelperAction::LaunchLatest => {
            let result = proxy_context().and_then(|(root, cfg)| {
                proxy::launch_version(&root, cfg.use_current_junction, None, false, &[])
                    .map_err(|cause| format!("启动应用失败：{cause:#}"))
            });
            match result {
                Ok(proxy::LaunchOutcome::Launched { .. }) => Some(0),
                Ok(proxy::LaunchOutcome::SwitchRequired { .. }) => None,
                Err(message) => {
                    dialogs::error(&format!(
                        "无法通过桌面快捷方式启动 ChatGPT：{message}\n\n将打开中文助手，你可以在概览页重试。"
                    ));
                    None
                }
            }
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--self-test") {
        return;
    }
    if let Some(exit_code) = run_cli_helper() {
        std::process::exit(exit_code);
    }

    let Some(_single_instance_guard) = claim_single_instance_or_focus_existing() else {
        return;
    };

    if let Ok(running) = std::env::current_exe() {
        if let Some(dir) = running.parent() {
            launcher_update::cleanup_stale_launchers(dir);
        }
    }

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
            launch_installed_codex,
            get_version_inventory,
            save_version_settings,
            set_desktop_shortcut,
            set_assistant_desktop_shortcut,
            set_codex_protocol,
            delete_installed_version,
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

#[cfg(test)]
mod cli_tests {
    use super::{parse_cli_helper, CliHelperAction};

    fn parse(args: &[&str]) -> Result<Option<CliHelperAction>, ()> {
        parse_cli_helper(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn parses_desktop_shortcut_and_launch_helpers() {
        assert_eq!(
            parse(&["--set-desktop-shortcut", "create"]),
            Ok(Some(CliHelperAction::SetDesktopShortcut(true)))
        );
        assert_eq!(
            parse(&["--set-desktop-shortcut", "remove"]),
            Ok(Some(CliHelperAction::SetDesktopShortcut(false)))
        );
        assert_eq!(
            parse(&["--set-assistant-desktop-shortcut", "create"]),
            Ok(Some(CliHelperAction::SetAssistantDesktopShortcut(true)))
        );
        assert_eq!(
            parse(&["--set-assistant-desktop-shortcut", "remove"]),
            Ok(Some(CliHelperAction::SetAssistantDesktopShortcut(false)))
        );
        assert_eq!(
            parse(&["--set-codex-protocol", "create"]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                enabled: true,
                replace_other: false,
            }))
        );
        assert_eq!(
            parse(&["--set-codex-protocol", "replace"]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                enabled: true,
                replace_other: true,
            }))
        );
        assert_eq!(
            parse(&["--set-codex-protocol", "remove"]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                enabled: false,
                replace_other: false,
            }))
        );
        assert_eq!(
            parse(&["--launch-latest"]),
            Ok(Some(CliHelperAction::LaunchLatest))
        );
    }

    #[test]
    fn parses_system_install_and_update_helpers() {
        assert_eq!(
            parse(&["--install-system", "request.json", "result.json"]),
            Ok(Some(CliHelperAction::InstallSystem {
                request_path: "request.json".into(),
                result_path: "result.json".into(),
            }))
        );
        assert_eq!(
            parse(&["--update-system", "result.json"]),
            Ok(Some(CliHelperAction::UpdateSystem {
                result_path: "result.json".into(),
            }))
        );
        assert_eq!(
            parse(&["--uninstall-system", "result.json"]),
            Ok(Some(CliHelperAction::UninstallSystem {
                result_path: "result.json".into(),
            }))
        );
        assert_eq!(
            parse(&["--uninstall"]),
            Ok(Some(CliHelperAction::Uninstall))
        );
    }

    #[test]
    fn rejects_invalid_helper_arguments() {
        assert_eq!(parse(&["--set-desktop-shortcut"]), Err(()));
        assert_eq!(parse(&["--set-assistant-desktop-shortcut"]), Err(()));
        assert_eq!(parse(&["--set-codex-protocol"]), Err(()));
        assert_eq!(parse(&["--launch-latest", "extra"]), Err(()));
        assert_eq!(parse(&["--install-system", "request.json"]), Err(()));
        assert_eq!(parse(&["--update-system"]), Err(()));
        assert_eq!(parse(&["--uninstall-system"]), Err(()));
        assert_eq!(parse(&["--uninstall", "extra"]), Err(()));
        assert_eq!(parse(&["--update-system", "result.json", "extra"]), Err(()));
    }
}
