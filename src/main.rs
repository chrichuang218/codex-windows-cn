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
    config::{
        capture_runtime_state, restore_runtime_state, Config, InstallMode, RuntimeStateBackup,
        CONFIG_FILENAME,
    },
    dialogs, elevate, installer, launcher_update, mode, proxy, registry, uninstall, updater,
    versions,
};
use serde::{Deserialize, Serialize};
use std::io::{Seek, SeekFrom, Write};
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
use windows::Win32::System::Threading::{
    CreateMutexW, ReleaseMutex, WaitForSingleObject, INFINITE,
};
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
static UPDATE_ACTIVE: AtomicBool = AtomicBool::new(false);

struct UpdateActivity;

impl UpdateActivity {
    fn begin() -> Result<Self, String> {
        UPDATE_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "更新已在进行中".to_string())?;
        Ok(Self)
    }
}

impl Drop for UpdateActivity {
    fn drop(&mut self) {
        UPDATE_ACTIVE.store(false, Ordering::Release);
    }
}

fn ensure_update_inactive(operation: &str) -> Result<(), String> {
    if UPDATE_ACTIVE.load(Ordering::Acquire) {
        return Err(format!("更新进行中，暂不能{operation}"));
    }
    Ok(())
}

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

#[cfg(windows)]
struct SystemMaintenanceGuard(HANDLE);

#[cfg(windows)]
impl Drop for SystemMaintenanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_system_maintenance() -> Result<SystemMaintenanceGuard, String> {
    let handle =
        unsafe { CreateMutexW(None, false, w!("Global\\CodexWindowsCnSystemMaintenance")) }
            .map_err(|cause| format!("创建系统维护锁失败：{cause}"))?;
    let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
    if wait.0 != 0 && wait.0 != 0x80 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(format!("等待系统维护锁失败：{}", wait.0));
    }
    Ok(SystemMaintenanceGuard(handle))
}

#[cfg(not(windows))]
struct SystemMaintenanceGuard;

#[cfg(not(windows))]
fn acquire_system_maintenance() -> Result<SystemMaintenanceGuard, String> {
    Ok(SystemMaintenanceGuard)
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
    if matches!(request.mode, BridgeInstallMode::System) && elevate::is_elevated() {
        return Err(
            "为确保同步发起用户的配置，请以普通用户身份重新启动中文助手后再执行所有用户安装".into(),
        );
    }
    let options = bridge::install_options_from_request(request.clone())?;
    let needs_elevation =
        matches!(request.mode, BridgeInstallMode::System) && !elevate::is_elevated();

    if let Ok(mut current) = install_event_state().lock() {
        *current = None;
    }

    if needs_elevation {
        let verify_codex_protocol = request.register_codex_protocol;
        let install_root = options.root.clone();
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
            let result = (|| -> Result<String, String> {
                let _system_guard = acquire_system_maintenance()?;
                run_elevated_task(
                    &format!(
                        "--install-system {} {}",
                        quote_cli_path(&request_path),
                        quote_cli_path(&result_path)
                    ),
                    &result_path,
                    "系统安装",
                )
                .and_then(|version| {
                    let installed =
                        Config::load(&install_root.join(CONFIG_FILENAME)).map_err(|cause| {
                            format!("系统安装已完成，但读取安装配置失败：{cause:#}")
                        })?;
                    installed.save_runtime(&install_root).map_err(|cause| {
                        format!("系统安装已完成，但同步当前用户配置失败：{cause:#}")
                    })?;
                    Ok(version)
                })
            })();
            let _ = std::fs::remove_file(&request_path);
            let _ = std::fs::remove_file(&result_path);
            let event = match result {
                Ok(version) => {
                    warn_if_current_user_codex_protocol_is_shadowed(
                        "系统安装",
                        &install_root,
                        verify_codex_protocol,
                    );
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
    ensure_update_inactive("修改版本与更新设置")?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CodexProtocolAction {
    Create,
    Replace,
    Remove,
}

#[derive(Debug, Deserialize, Serialize)]
struct CodexProtocolRollbackPayload {
    install_root: PathBuf,
    protocol: registry::CodexProtocolBackup,
    install_config: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CodexProtocolRestorePayload {
    original: CodexProtocolRollbackPayload,
    expected_protocol: registry::CodexProtocolBackup,
    expected_install_config: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CodexProtocolAppliedState {
    protocol: registry::CodexProtocolBackup,
    install_config: Vec<u8>,
}

struct SystemCodexProtocolRollback {
    system: CodexProtocolRollbackPayload,
    encoded_system: String,
    current_user_protocol: registry::CodexProtocolBackup,
    runtime_state: RuntimeStateBackup,
}

#[derive(Clone, Copy)]
enum CurrentUserRollback {
    None,
    RuntimeState,
    ProtocolAndRuntimeState,
}

struct CodexProtocolChangeRollback {
    protocol: registry::CodexProtocolBackup,
    install_config: Vec<u8>,
}

impl CodexProtocolAction {
    fn enabled(self) -> bool {
        self != Self::Remove
    }

    fn replace_other(self) -> bool {
        self == Self::Replace
    }

    fn helper_argument(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Remove => "remove",
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            Self::Create => "已创建或修复 codex:// 会话链接",
            Self::Replace => "已将 codex:// 会话链接切换到当前安装",
            Self::Remove => "已停止由当前安装处理 codex:// 会话链接",
        }
    }
}

fn capture_system_codex_protocol_rollback(
    root: &Path,
) -> Result<SystemCodexProtocolRollback, String> {
    let payload = CodexProtocolRollbackPayload {
        install_root: root.to_path_buf(),
        protocol: registry::capture_codex_protocol(InstallMode::System)
            .map_err(|cause| format!("备份系统级 codex:// 注册失败：{cause:#}"))?,
        install_config: std::fs::read(root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("备份系统安装配置失败：{cause}"))?,
    };
    let raw = serde_json::to_vec(&payload)
        .map_err(|cause| format!("序列化 codex:// 回滚数据失败：{cause}"))?;
    let encoded = launcher_update::hex_encode(&raw);
    if encoded.len() > 24_000 {
        return Err("codex:// 回滚数据过大，已停止系统级修改".into());
    }
    Ok(SystemCodexProtocolRollback {
        system: payload,
        encoded_system: encoded,
        current_user_protocol: registry::capture_codex_protocol(InstallMode::User)
            .map_err(|cause| format!("备份当前用户的 codex:// 注册失败：{cause:#}"))?,
        runtime_state: capture_runtime_state()
            .map_err(|cause| format!("备份当前用户运行配置失败：{cause}"))?,
    })
}

fn capture_codex_protocol_change_rollback(
    root: &Path,
    mode: InstallMode,
) -> Result<CodexProtocolChangeRollback, String> {
    Ok(CodexProtocolChangeRollback {
        protocol: registry::capture_codex_protocol(mode)
            .map_err(|cause| format!("备份 codex:// 注册失败：{cause:#}"))?,
        install_config: std::fs::read(root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("备份安装配置失败：{cause}"))?,
    })
}

fn rollback_codex_protocol_registry(
    mode: InstallMode,
    rollback: &CodexProtocolChangeRollback,
    cause: impl Into<String>,
) -> String {
    let cause = cause.into();
    match registry::restore_codex_protocol(mode, &rollback.protocol) {
        Ok(()) => cause,
        Err(error) => format!("{cause}；恢复 codex:// 注册失败：{error:#}"),
    }
}

fn rollback_codex_protocol_change(
    root: &Path,
    mode: InstallMode,
    rollback: &CodexProtocolChangeRollback,
    cause: impl Into<String>,
) -> String {
    let cause = rollback_codex_protocol_registry(mode, rollback, cause);
    let mut failures = Vec::new();
    if let Err(error) = std::fs::write(root.join(CONFIG_FILENAME), &rollback.install_config) {
        failures.push(format!("恢复安装配置失败：{error}"));
    }
    if failures.is_empty() {
        cause
    } else {
        format!("{cause}；回滚未完整完成：{}", failures.join("；"))
    }
}

fn restore_system_codex_protocol(encoded: &str) -> Result<(), String> {
    if !elevate::is_elevated() {
        return Err("恢复系统级 codex:// 注册需要管理员权限".into());
    }
    let raw = hex_decode(encoded)?;
    let payload: CodexProtocolRestorePayload = serde_json::from_slice(&raw)
        .map_err(|cause| format!("解析 codex:// 回滚数据失败：{cause}"))?;
    restore_system_codex_protocol_payload(&payload)
}

fn restore_system_codex_protocol_payload(
    payload: &CodexProtocolRestorePayload,
) -> Result<(), String> {
    let root = mode::install_root().map_err(|cause| format!("读取安装目录失败：{cause:#}"))?;
    let install_config: Config = serde_json::from_slice(&payload.original.install_config)
        .map_err(|cause| format!("回滚数据中的系统安装配置无效：{cause}"))?;
    if root != payload.original.install_root
        || !matches!(install_config.install_mode, InstallMode::System)
    {
        return Err("codex:// 回滚数据不属于当前系统安装".into());
    }
    let current_install_config = std::fs::read(root.join(CONFIG_FILENAME))
        .map_err(|cause| format!("核验系统安装配置失败：{cause}"))?;
    if current_install_config != payload.expected_install_config {
        return Err("系统安装配置已被其他进程修改，已跳过回滚".into());
    }
    if !registry::restore_codex_protocol_if_unchanged(
        InstallMode::System,
        &payload.expected_protocol,
        &payload.original.protocol,
    )
    .map_err(|cause| format!("恢复系统级 codex:// 注册失败：{cause:#}"))?
    {
        return Err("系统级 codex:// 注册已被其他进程修改，已跳过回滚".into());
    }
    let current_install_config = std::fs::read(root.join(CONFIG_FILENAME))
        .map_err(|cause| format!("再次核验系统安装配置失败：{cause}"))?;
    if current_install_config != payload.expected_install_config {
        return Err("系统安装配置在回滚期间被其他进程修改，已跳过配置恢复".into());
    }
    std::fs::write(root.join(CONFIG_FILENAME), &payload.original.install_config)
        .map_err(|cause| format!("恢复系统安装配置失败：{cause}"))
}

fn rollback_system_codex_protocol(
    rollback: &SystemCodexProtocolRollback,
    expected: &CodexProtocolAppliedState,
    current_user: CurrentUserRollback,
    cause: impl Into<String>,
) -> String {
    let cause = cause.into();
    let mut failures = Vec::new();
    let elevated_payload = (|| -> Result<String, String> {
        let payload = CodexProtocolRestorePayload {
            original: CodexProtocolRollbackPayload {
                install_root: rollback.system.install_root.clone(),
                protocol: rollback.system.protocol.clone(),
                install_config: rollback.system.install_config.clone(),
            },
            expected_protocol: expected.protocol.clone(),
            expected_install_config: expected.install_config.clone(),
        };
        let raw = serde_json::to_vec(&payload)
            .map_err(|error| format!("序列化 codex:// 恢复数据失败：{error}"))?;
        let encoded = launcher_update::hex_encode(&raw);
        if encoded.len() > 24_000 {
            return Err("codex:// 恢复数据过大，无法安全回滚".into());
        }
        Ok(encoded)
    })();
    let elevated_payload = match elevated_payload {
        Ok(payload) => payload,
        Err(error) => {
            failures.push(error);
            String::new()
        }
    };
    match (!elevated_payload.is_empty()).then(|| {
        elevate::respawn_elevated_wait(&format!("--restore-codex-protocol {elevated_payload}"))
    }) {
        None => {}
        Some(result) => match result {
            Ok(0) => {}
            Ok(exit_code) => failures.push(format!(
                "恢复系统级 codex:// 状态失败（退出代码 {exit_code}）"
            )),
            Err(error) => failures.push(format!("恢复系统级 codex:// 状态失败：{error:#}")),
        },
    }
    if matches!(current_user, CurrentUserRollback::ProtocolAndRuntimeState) {
        if let Err(error) =
            registry::restore_codex_protocol(InstallMode::User, &rollback.current_user_protocol)
        {
            failures.push(format!("恢复当前用户 codex:// 状态失败：{error:#}"));
        }
    }
    if !matches!(current_user, CurrentUserRollback::None) {
        if let Err(error) = restore_runtime_state(&rollback.runtime_state) {
            failures.push(format!("恢复当前用户运行配置失败：{error:#}"));
        }
    }
    if failures.is_empty() {
        cause
    } else {
        format!("{cause}；{}", failures.join("；"))
    }
}

fn hex_decode(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() % 2 != 0 {
        return Err("codex:// 回滚数据长度无效".into());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| "codex:// 回滚数据包含无效字符".to_string())?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| "codex:// 回滚数据包含无效字符".to_string())?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

#[tauri::command]
fn set_codex_protocol(action: CodexProtocolAction) -> Result<CodexProtocolActionResult, String> {
    let _guard = lock_runtime_config()?;
    ensure_update_inactive("修改 codex:// 会话链接")?;
    let (root, mut cfg) = proxy_context()?;
    let message = if matches!(cfg.install_mode, InstallMode::System) {
        if elevate::is_elevated() {
            return Err(
                "为避免操作到提权账户的用户关联，请以普通用户身份重新启动中文助手后再修改 codex://"
                    .into(),
            );
        }
        apply_system_codex_protocol_change(&root, &mut cfg, action)?
    } else {
        apply_codex_protocol_change(&root, &mut cfg, action, false)?
    };

    let inventory = bridge::version_inventory(&root, &cfg)
        .map_err(|cause| format!("刷新 codex:// 会话链接状态失败：{cause:#}"))?;
    Ok(CodexProtocolActionResult {
        applied: true,
        message,
        inventory,
    })
}

fn apply_system_codex_protocol_change(
    root: &Path,
    cfg: &mut Config,
    action: CodexProtocolAction,
) -> Result<String, String> {
    let _system_guard = acquire_system_maintenance()?;
    if action.enabled() {
        let current_status = installer::codex_protocol_status(root, cfg)
            .map_err(|cause| format!("读取当前用户的 codex:// 会话链接失败：{cause:#}"))?;
        if matches!(
            current_status,
            codex_windows_cn::registry::CodexProtocolStatus::Ready
        ) {
            cfg.register_codex_protocol = Some(true);
            cfg.save_runtime(root)
                .map_err(|cause| format!("保存 codex:// 会话链接偏好失败：{cause:#}"))?;
            return Ok(action.success_message().to_string());
        }
        if !registry::system_codex_protocol_replace_is_safe_for_current_user(root)
            .map_err(|cause| format!("检查系统级 codex:// 接管范围失败：{cause:#}"))?
        {
            return Err(
                "当前 Windows 用户已有 codex:// 覆盖项，系统级注册无法安全接管；请先在 Windows 默认应用中移除现有关联"
                    .into(),
            );
        }
    }

    let rollback = capture_system_codex_protocol_rollback(root)?;
    let result_path = elevated_temp_path("protocol-result");
    let applied = run_elevated_task(
        &format!(
            "--set-codex-protocol {} {} {}",
            action.helper_argument(),
            quote_cli_path(&result_path),
            rollback.encoded_system
        ),
        &result_path,
        "系统级 codex:// 配置",
    );
    let _ = std::fs::remove_file(&result_path);
    let applied: CodexProtocolAppliedState = serde_json::from_str(&applied?)
        .map_err(|cause| format!("管理员协议配置返回的写后状态无效：{cause}"))?;

    if action.enabled() {
        match installer::codex_protocol_status(root, cfg) {
            Ok(codex_windows_cn::registry::CodexProtocolStatus::Ready) => {}
            Ok(_) => {
                return Err(rollback_system_codex_protocol(
                    &rollback,
                    &applied,
                    CurrentUserRollback::None,
                    "授权期间当前用户的 codex:// 默认关联发生变化，系统级注册未能成为实际处理程序",
                ));
            }
            Err(cause) => {
                return Err(rollback_system_codex_protocol(
                    &rollback,
                    &applied,
                    CurrentUserRollback::None,
                    format!("核验当前用户的 codex:// 会话链接失败：{cause:#}"),
                ));
            }
        }
    }

    cfg.register_codex_protocol = Some(action.enabled());
    if let Err(cause) = cfg.save_runtime(root) {
        return Err(rollback_system_codex_protocol(
            &rollback,
            &applied,
            CurrentUserRollback::RuntimeState,
            format!("保存 codex:// 会话链接偏好失败：{cause:#}"),
        ));
    }
    if !action.enabled() {
        if let Err(cause) = registry::remove_codex_protocol_if_owned(InstallMode::User, root) {
            return Err(rollback_system_codex_protocol(
                &rollback,
                &applied,
                CurrentUserRollback::ProtocolAndRuntimeState,
                format!("清理当前用户的 codex:// 会话链接失败：{cause:#}"),
            ));
        }
    }
    Ok(action.success_message().to_string())
}

fn apply_codex_protocol_change(
    root: &Path,
    cfg: &mut Config,
    action: CodexProtocolAction,
    persist_install: bool,
) -> Result<String, String> {
    let rollback = capture_codex_protocol_change_rollback(root, cfg.install_mode)?;
    if action.enabled() {
        let registration = installer::enable_codex_protocol(root, cfg, action.replace_other())
            .map_err(|cause| {
                rollback_codex_protocol_registry(
                    cfg.install_mode,
                    &rollback,
                    format!("配置 codex:// 会话链接失败：{cause:#}"),
                )
            })?;
        if matches!(
            registration,
            codex_windows_cn::registry::ProtocolRegistration::PreservedForeign
        ) {
            return Err(
                "codex:// 当前由其他 Codex 安装处理；如需切换，请使用“设为当前安装”".into(),
            );
        }
    } else {
        let _ = installer::disable_codex_protocol(root, cfg).map_err(|cause| {
            rollback_codex_protocol_registry(
                cfg.install_mode,
                &rollback,
                format!("移除 codex:// 会话链接失败：{cause:#}"),
            )
        })?;
    }

    if action.enabled() && !persist_install {
        match installer::codex_protocol_status(root, cfg) {
            Ok(codex_windows_cn::registry::CodexProtocolStatus::Ready) => {}
            Ok(status) => {
                return Err(rollback_codex_protocol_registry(
                    cfg.install_mode,
                    &rollback,
                    format!("codex:// 注册未成为当前有效处理程序，实际状态：{status:?}"),
                ));
            }
            Err(cause) => {
                return Err(rollback_codex_protocol_registry(
                    cfg.install_mode,
                    &rollback,
                    format!("核验 codex:// 当前有效处理程序失败：{cause:#}"),
                ));
            }
        }
    }

    cfg.register_codex_protocol = Some(action.enabled());
    let save_result = if persist_install {
        cfg.save(&root.join(CONFIG_FILENAME))
    } else {
        cfg.save_runtime(root)
    };
    if let Err(cause) = save_result {
        return Err(rollback_codex_protocol_change(
            root,
            cfg.install_mode,
            &rollback,
            format!("保存 codex:// 会话链接偏好失败：{cause:#}"),
        ));
    }
    Ok(action.success_message().to_string())
}

fn apply_system_codex_protocol_change_with_applied(
    root: &Path,
    cfg: &mut Config,
    action: CodexProtocolAction,
) -> Result<(String, CodexProtocolAppliedState), String> {
    let rollback = capture_codex_protocol_change_rollback(root, cfg.install_mode)?;
    let protocol = if action.enabled() {
        let (registration, protocol) =
            installer::enable_codex_protocol_with_backup(root, cfg, action.replace_other())
                .map_err(|cause| {
                    rollback_codex_protocol_registry(
                        cfg.install_mode,
                        &rollback,
                        format!("配置 codex:// 会话链接失败：{cause:#}"),
                    )
                })?;
        if matches!(
            registration,
            codex_windows_cn::registry::ProtocolRegistration::PreservedForeign
        ) {
            return Err(
                "codex:// 当前由其他 Codex 安装处理；如需切换，请使用“设为当前安装”".into(),
            );
        }
        protocol
    } else {
        let (_, protocol) =
            installer::disable_codex_protocol_with_backup(root, cfg).map_err(|cause| {
                rollback_codex_protocol_registry(
                    cfg.install_mode,
                    &rollback,
                    format!("移除 codex:// 会话链接失败：{cause:#}"),
                )
            })?;
        protocol
    };

    cfg.register_codex_protocol = Some(action.enabled());
    let install_config = serde_json::to_vec_pretty(cfg).map_err(|cause| {
        rollback_codex_protocol_registry(
            cfg.install_mode,
            &rollback,
            format!("准备 codex:// 安装配置失败：{cause}"),
        )
    })?;
    if let Err(cause) = cfg.save(&root.join(CONFIG_FILENAME)) {
        return Err(rollback_codex_protocol_change(
            root,
            cfg.install_mode,
            &rollback,
            format!("保存 codex:// 会话链接偏好失败：{cause:#}"),
        ));
    }
    Ok((
        action.success_message().to_string(),
        CodexProtocolAppliedState {
            protocol,
            install_config,
        },
    ))
}

#[tauri::command]
fn delete_installed_version(version: String) -> Result<VersionActionResult, String> {
    let _guard = lock_runtime_config()?;
    ensure_update_inactive("删除已安装版本")?;
    let (root, mut cfg) = proxy_context()?;
    if !versions::is_version_name(&version) {
        return Err("版本号格式无效".into());
    }
    if matches!(cfg.install_mode, InstallMode::System) {
        if elevate::is_elevated() {
            return Err(
                "为避免读取提权账户的版本设置，请以普通用户身份重新启动中文助手后再删除版本".into(),
            );
        }
        let (request_path, result_path) =
            write_elevated_version_delete_request(&root, &cfg, &version)?;
        let _system_guard = acquire_system_maintenance()?;
        let result = run_elevated_task(
            &format!(
                "--delete-installed-version {} {}",
                quote_cli_path(&request_path),
                quote_cli_path(&result_path)
            ),
            &result_path,
            "删除系统版本",
        );
        let _ = std::fs::remove_file(&request_path);
        let _ = std::fs::remove_file(&result_path);
        let message = result?;
        let install_config = Config::load(&root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("版本已删除，但读取新配置失败：{cause:#}"))?;
        cfg = Config::load_runtime(&root)
            .map_err(|cause| format!("重新读取版本状态失败：{cause:#}"))?;
        cfg.current_version = install_config.current_version;
        cfg.save_runtime(&root)
            .map_err(|cause| format!("版本已删除，但同步当前用户配置失败：{cause:#}"))?;
        let inventory = bridge::version_inventory(&root, &cfg)
            .map_err(|cause| format!("刷新已安装版本失败：{cause:#}"))?;
        return Ok(VersionActionResult {
            applied: true,
            message,
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
    ensure_update_inactive("修改更新提醒设置")?;
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
    let runtime_guard = lock_runtime_config()?;
    let (root, cfg) = proxy_context()?;
    let system_install = matches!(cfg.install_mode, InstallMode::System);

    if system_install && elevate::is_elevated() {
        return Err(
            "为避免读取提权账户的更新设置，请以普通用户身份重新启动中文助手后再更新".into(),
        );
    }
    let update_activity = UpdateActivity::begin()?;
    drop(runtime_guard);

    if let Ok(mut current) = update_event_state().lock() {
        *current = None;
    }

    if system_install {
        let verify_codex_protocol = cfg.register_codex_protocol_enabled();
        let (request_path, result_path) = write_elevated_update_request(&root, &cfg)?;
        publish_update_event(
            &app,
            bridge::update_event_from_msg(installer::InstallMsg::Phase {
                phase: "Elevating".into(),
                detail: "请在 Windows 提示中允许管理员权限。".into(),
            }),
        );
        std::thread::spawn(move || {
            let _update_activity = update_activity;
            let result = (|| -> Result<String, String> {
                let _system_guard = acquire_system_maintenance()?;
                run_elevated_task(
                    &format!(
                        "--update-system {} {}",
                        quote_cli_path(&request_path),
                        quote_cli_path(&result_path)
                    ),
                    &result_path,
                    "系统更新",
                )
                .and_then(|version| {
                    let updated_install = Config::load(&root.join(CONFIG_FILENAME))
                        .map_err(|cause| format!("系统更新已完成，但读取新配置失败：{cause:#}"))?;
                    let mut current_user = Config::load_runtime(&root).map_err(|cause| {
                        format!("系统更新已完成，但读取当前用户配置失败：{cause:#}")
                    })?;
                    current_user.current_version = updated_install.current_version;
                    current_user.known_latest = updated_install.known_latest;
                    current_user.suppress_until_unix = updated_install.suppress_until_unix;
                    current_user.last_check_unix = updated_install.last_check_unix;
                    current_user.save_runtime(&root).map_err(|cause| {
                        format!("系统更新已完成，但同步当前用户配置失败：{cause:#}")
                    })?;
                    Ok(version)
                })
            })();
            let _ = std::fs::remove_file(&request_path);
            let _ = std::fs::remove_file(&result_path);
            let event = match result {
                Ok(version) => {
                    warn_if_current_user_codex_protocol_is_shadowed(
                        "系统更新",
                        &root,
                        verify_codex_protocol,
                    );
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
        let _update_activity = update_activity;
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
    let _guard = lock_runtime_config()?;
    ensure_update_inactive("卸载")?;
    let ctx =
        uninstall::load_context().map_err(|cause| format!("无法读取卸载上下文：{cause:#}"))?;

    if matches!(ctx.cfg.install_mode, InstallMode::System) && elevate::is_elevated() {
        return Err(
            "为确保清理发起卸载用户的 codex:// 关联，请以普通用户身份重新启动中文助手后再卸载"
                .into(),
        );
    }

    if let Ok(mut current) = uninstall_event_state().lock() {
        *current = None;
    }

    if uninstall::need_elevation(&ctx) {
        let result_path = elevated_temp_path("uninstall-result");
        let install_root = ctx.root.clone();
        publish_uninstall_event(
            &app,
            bridge::uninstall_event_from_msg(uninstall::UninstallMsg::Phase {
                phase: "Elevating".into(),
                detail: "请在 Windows 提示中允许管理员权限。".into(),
            }),
        );
        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                let _system_guard = acquire_system_maintenance()?;
                run_elevated_task(
                    &format!("--uninstall-system {}", quote_cli_path(&result_path)),
                    &result_path,
                    "系统卸载",
                )
                .and_then(|log_path| {
                    registry::remove_codex_protocol_if_owned(InstallMode::User, &install_root)
                        .map_err(|cause| {
                            format!(
                                "系统卸载已完成，但清理当前用户的 codex:// 会话链接失败：{cause:#}"
                            )
                        })?;
                    Ok(log_path)
                })
            })();
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

fn warn_if_current_user_codex_protocol_is_shadowed(
    operation: &str,
    install_root: &Path,
    should_be_enabled: bool,
) {
    if !should_be_enabled {
        return;
    }
    let cfg = match Config::load_runtime(install_root) {
        Ok(cfg) => cfg,
        Err(cause) => {
            dialogs::error(&format!(
                "{operation}已完成，但无法读取目标安装的 codex:// 会话链接状态：{cause:#}"
            ));
            return;
        }
    };
    match installer::codex_protocol_status(install_root, &cfg) {
        Ok(codex_windows_cn::registry::CodexProtocolStatus::Ready) => {}
        Ok(_) => dialogs::error(&format!(
            "{operation}已完成，但当前 Windows 用户的 codex:// 默认关联仍由其他应用覆盖。\n\n可在中文助手设置中查看状态，或先到 Windows 默认应用中移除现有关联。"
        )),
        Err(cause) => dialogs::error(&format!(
            "{operation}已完成，但无法核验当前 Windows 用户的 codex:// 会话链接：{cause:#}"
        )),
    }
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

enum ElevatedTaskRead {
    Pending,
    Success(String),
    Error(String),
}

const ELEVATED_TASK_PENDING_MESSAGE: &str = "管理员操作未能返回最终结果";

#[derive(Debug, Serialize, Deserialize)]
struct SystemUpdateRequest {
    install_root: PathBuf,
    runtime_config: Config,
}

#[derive(Debug, Serialize, Deserialize)]
struct SystemVersionDeleteRequest {
    install_root: PathBuf,
    runtime_config: Config,
    version: String,
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

fn write_elevated_update_request(
    install_root: &Path,
    runtime_config: &Config,
) -> Result<(PathBuf, PathBuf), String> {
    let request_path = elevated_temp_path("update-request");
    let result_path = elevated_temp_path("update-result");
    let raw = serde_json::to_vec(&SystemUpdateRequest {
        install_root: install_root.to_path_buf(),
        runtime_config: runtime_config.clone(),
    })
    .map_err(|cause| format!("准备系统更新请求失败：{cause}"))?;
    std::fs::write(&request_path, raw).map_err(|cause| format!("写入系统更新请求失败：{cause}"))?;
    Ok((request_path, result_path))
}

fn write_elevated_version_delete_request(
    install_root: &Path,
    runtime_config: &Config,
    version: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let request_path = elevated_temp_path("version-delete-request");
    let result_path = elevated_temp_path("version-delete-result");
    let raw = serde_json::to_vec(&SystemVersionDeleteRequest {
        install_root: install_root.to_path_buf(),
        runtime_config: runtime_config.clone(),
        version: version.to_string(),
    })
    .map_err(|cause| format!("准备系统版本删除请求失败：{cause}"))?;
    std::fs::write(&request_path, raw)
        .map_err(|cause| format!("写入系统版本删除请求失败：{cause}"))?;
    Ok((request_path, result_path))
}

fn quote_cli_path(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn run_elevated_task(args: &str, result_path: &Path, operation: &str) -> Result<String, String> {
    let exit_code = match elevate::respawn_elevated_wait_with_state(args) {
        Ok(exit_code) => Some(exit_code),
        Err(failure) if !failure.launched => {
            return Err(format!(
                "{operation}未获得管理员权限，操作未开始：{:#}",
                failure.cause
            ));
        }
        Err(failure) if failure.finished => None,
        Err(failure) => {
            let deadline = std::time::Instant::now() + Duration::from_secs(300);
            let mut last_result_error = None;
            loop {
                match read_elevated_task_result(result_path, operation, None) {
                    Ok(ElevatedTaskRead::Success(value)) => return Ok(value),
                    Ok(ElevatedTaskRead::Error(message)) => return Err(message),
                    Ok(ElevatedTaskRead::Pending) => {}
                    Err(message) => last_result_error = Some(message),
                }
                if std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                let detail = last_result_error
                    .map(|message| format!("；最后一次读取结果失败：{message}"))
                    .unwrap_or_default();
                return Err(format!(
                    "{operation}已启动，但无法确认管理员进程是否结束：{:#}{detail}",
                    failure.cause
                ));
            }
        }
    };
    match read_elevated_task_result(result_path, operation, exit_code)? {
        ElevatedTaskRead::Success(value) => Ok(value),
        ElevatedTaskRead::Error(message) => Err(message),
        ElevatedTaskRead::Pending => {
            Err(format!("{operation}进程结束，但仍只返回了未完成占位结果"))
        }
    }
}

fn read_elevated_task_result(
    result_path: &Path,
    operation: &str,
    exit_code: Option<u32>,
) -> Result<ElevatedTaskRead, String> {
    let raw = std::fs::read(result_path).map_err(|cause| match exit_code {
        Some(exit_code) => {
            format!("{operation}进程未返回结果（退出代码 {exit_code}）：{cause}")
        }
        None => format!("{operation}进程未返回结果：{cause}"),
    })?;
    let result: ElevatedTaskResult =
        serde_json::from_slice(&raw).map_err(|cause| match exit_code {
            Some(exit_code) => format!("{operation}结果无效（退出代码 {exit_code}）：{cause}"),
            None => format!("{operation}结果无效：{cause}"),
        })?;
    match result {
        ElevatedTaskResult::Success { value } if exit_code.unwrap_or(0) == 0 => {
            Ok(ElevatedTaskRead::Success(value))
        }
        ElevatedTaskResult::Success { .. } => Ok(ElevatedTaskRead::Error(format!(
            "{operation}进程异常退出（退出代码 {}）",
            exit_code.expect("non-zero exit code is present")
        ))),
        ElevatedTaskResult::Error { message } if message == ELEVATED_TASK_PENDING_MESSAGE => {
            Ok(ElevatedTaskRead::Pending)
        }
        ElevatedTaskResult::Error { message } => Ok(ElevatedTaskRead::Error(message)),
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

fn prepare_elevated_result_file(path: &Path) -> Result<std::fs::File, String> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|cause| format!("打开管理员结果文件失败：{cause}"))?;
    let placeholder = ElevatedTaskResult::Error {
        message: ELEVATED_TASK_PENDING_MESSAGE.into(),
    };
    let raw = serde_json::to_vec(&placeholder)
        .map_err(|cause| format!("序列化管理员结果占位失败：{cause}"))?;
    file.write_all(&raw)
        .map_err(|cause| format!("预写管理员结果文件失败：{cause}"))?;
    file.sync_all()
        .map_err(|cause| format!("刷新管理员结果文件失败：{cause}"))?;
    Ok(file)
}

fn finish_elevated_task_file(mut file: std::fs::File, result: Result<String, String>) -> i32 {
    let succeeded = result.is_ok();
    let payload = match result {
        Ok(value) => ElevatedTaskResult::Success { value },
        Err(message) => ElevatedTaskResult::Error { message },
    };
    let write_result = (|| -> Result<(), String> {
        let raw = serde_json::to_vec(&payload)
            .map_err(|cause| format!("序列化管理员结果失败：{cause}"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|cause| format!("定位管理员结果文件失败：{cause}"))?;
        file.write_all(&raw)
            .map_err(|cause| format!("写入管理员结果失败：{cause}"))?;
        file.set_len(raw.len() as u64)
            .map_err(|cause| format!("截断管理员结果文件失败：{cause}"))?;
        file.sync_all()
            .map_err(|cause| format!("刷新管理员结果失败：{cause}"))
    })();
    if let Err(message) = write_result {
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

fn finish_system_codex_protocol_task(
    file: std::fs::File,
    result: Result<CodexProtocolAppliedState, String>,
    rollback: &CodexProtocolRollbackPayload,
) -> i32 {
    match result {
        Ok(applied) => {
            let value = match serde_json::to_string(&applied) {
                Ok(value) => value,
                Err(cause) => {
                    let restore = CodexProtocolRestorePayload {
                        original: CodexProtocolRollbackPayload {
                            install_root: rollback.install_root.clone(),
                            protocol: rollback.protocol.clone(),
                            install_config: rollback.install_config.clone(),
                        },
                        expected_protocol: applied.protocol,
                        expected_install_config: applied.install_config,
                    };
                    let rollback_error = restore_system_codex_protocol_payload(&restore).err();
                    let message = format!("序列化 codex:// 写后状态失败：{cause}");
                    dialogs::error(&match rollback_error {
                        Some(error) => format!("{message}；回滚失败：{error}"),
                        None => message,
                    });
                    return 1;
                }
            };
            let restore = CodexProtocolRestorePayload {
                original: CodexProtocolRollbackPayload {
                    install_root: rollback.install_root.clone(),
                    protocol: rollback.protocol.clone(),
                    install_config: rollback.install_config.clone(),
                },
                expected_protocol: applied.protocol,
                expected_install_config: applied.install_config,
            };
            let exit_code = finish_elevated_task_file(file, Ok(value));
            if exit_code == 0 {
                return 0;
            }
            let rollback_error = restore_system_codex_protocol_payload(&restore).err();
            if let Some(error) = rollback_error {
                dialogs::error(&format!(
                    "管理员结果无法持久化，且 codex:// 系统级修改回滚失败：{error}"
                ));
            }
            1
        }
        Err(message) => finish_elevated_task_file(file, Err(message)),
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

fn run_system_update_helper(request_path: &Path, result_path: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        if !elevate::is_elevated() {
            return Err("系统更新需要管理员权限".into());
        }
        let raw = std::fs::read(request_path)
            .map_err(|cause| format!("读取系统更新请求失败：{cause}"))?;
        let request: SystemUpdateRequest = serde_json::from_slice(&raw)
            .map_err(|cause| format!("解析系统更新请求失败：{cause}"))?;
        let root = mode::install_root().map_err(|cause| format!("无法读取安装目录：{cause:#}"))?;
        if root != request.install_root
            || !matches!(request.runtime_config.install_mode, InstallMode::System)
        {
            return Err("当前安装不是所有用户安装，无法执行系统更新".into());
        }
        installer::update_elevated(root, request.runtime_config)
            .map_err(|cause| format!("系统更新失败：{cause:#}"))
    })();
    finish_elevated_task(result_path, result)
}

fn run_system_version_delete_helper(request_path: &Path, result_path: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        if !elevate::is_elevated() {
            return Err("删除系统版本需要管理员权限".into());
        }
        let raw = std::fs::read(request_path)
            .map_err(|cause| format!("读取系统版本删除请求失败：{cause}"))?;
        let request: SystemVersionDeleteRequest = serde_json::from_slice(&raw)
            .map_err(|cause| format!("解析系统版本删除请求失败：{cause}"))?;
        let root = mode::install_root().map_err(|cause| format!("无法读取安装目录：{cause:#}"))?;
        if root != request.install_root
            || !matches!(request.runtime_config.install_mode, InstallMode::System)
            || !versions::is_version_name(&request.version)
        {
            return Err("系统版本删除请求与当前安装不匹配".into());
        }

        let mut runtime_config = request.runtime_config;
        let running = proxy::running_versions(&root.join("versions"));
        let repair = versions::delete_and_repair_in_memory(
            &root,
            &mut runtime_config,
            &request.version,
            &running,
        )
        .map_err(|cause| format!("删除版本失败：{cause:#}"))?;
        let mut install_config = Config::load(&root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("读取系统安装配置失败：{cause:#}"))?;
        install_config.current_version = runtime_config.current_version.clone();
        install_config
            .save(&root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("保存系统安装配置失败：{cause:#}"))?;
        runtime_config.register_codex_protocol = install_config.register_codex_protocol;
        let protocol_warning = installer::sync_codex_protocol(&root, &runtime_config)
            .err()
            .map(|cause| format!("；codex:// 会话链接刷新失败：{cause:#}"))
            .unwrap_or_default();
        Ok(if repair.current_repaired {
            format!("已删除版本 {}{protocol_warning}", request.version)
        } else {
            format!(
                "已删除版本 {}；current 入口将在下次启动时重试修复到 {}{}",
                request.version, repair.default_version, protocol_warning
            )
        })
    })();
    finish_elevated_task(result_path, result)
}

fn run_system_codex_protocol_helper(
    action: CodexProtocolAction,
    result_path: &Path,
    encoded_rollback: &str,
) -> i32 {
    let result_file = match prepare_elevated_result_file(result_path) {
        Ok(file) => file,
        Err(message) => {
            dialogs::error(&message);
            return 1;
        }
    };
    let rollback = match (|| -> Result<CodexProtocolRollbackPayload, String> {
        if !elevate::is_elevated() {
            return Err("系统级 codex:// 配置需要管理员权限".into());
        }
        let raw = hex_decode(encoded_rollback)?;
        serde_json::from_slice(&raw).map_err(|cause| format!("解析 codex:// 原始状态失败：{cause}"))
    })() {
        Ok(rollback) => rollback,
        Err(message) => return finish_elevated_task_file(result_file, Err(message)),
    };
    let result = (|| -> Result<CodexProtocolAppliedState, String> {
        let root = mode::install_root().map_err(|cause| format!("无法读取安装目录：{cause:#}"))?;
        let mut cfg = Config::load(&root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("无法读取系统安装配置：{cause:#}"))?;
        if root != rollback.install_root || !matches!(cfg.install_mode, InstallMode::System) {
            return Err("codex:// 原始状态不属于当前系统安装".into());
        }
        let current_config = std::fs::read(root.join(CONFIG_FILENAME))
            .map_err(|cause| format!("核验系统安装配置失败：{cause}"))?;
        if current_config != rollback.install_config {
            return Err("系统安装配置在授权期间发生变化，已停止修改 codex://".into());
        }
        let current_protocol = registry::capture_codex_protocol(InstallMode::System)
            .map_err(|cause| format!("核验系统级 codex:// 原始状态失败：{cause:#}"))?;
        if current_protocol != rollback.protocol {
            return Err("系统级 codex:// 注册在授权期间发生变化，已停止修改".into());
        }
        let (_, applied) =
            apply_system_codex_protocol_change_with_applied(&root, &mut cfg, action)?;
        Ok(applied)
    })();
    finish_system_codex_protocol_task(result_file, result, &rollback)
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

        uninstall::run_worker_to_completion(ctx, |_| {})
    })();
    finish_elevated_task(result_path, result)
}

#[derive(Debug, PartialEq, Eq)]
enum CliHelperAction {
    Uninstall,
    UninstallElevated,
    DeleteInstalledVersion {
        request_path: String,
        result_path: String,
    },
    SetDesktopShortcut(bool),
    SetAssistantDesktopShortcut(bool),
    SetCodexProtocol {
        action: CodexProtocolAction,
        result_path: String,
        rollback: String,
    },
    RestoreCodexProtocol(String),
    InstallSystem {
        request_path: String,
        result_path: String,
    },
    UpdateSystem {
        request_path: String,
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
        "--uninstall-elevated" => CliHelperAction::UninstallElevated,
        "--delete-installed-version" => CliHelperAction::DeleteInstalledVersion {
            request_path: args.next().ok_or(())?,
            result_path: args.next().ok_or(())?,
        },
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
                action: CodexProtocolAction::Create,
                result_path: args.next().ok_or(())?,
                rollback: args.next().ok_or(())?,
            },
            Some("replace") => CliHelperAction::SetCodexProtocol {
                action: CodexProtocolAction::Replace,
                result_path: args.next().ok_or(())?,
                rollback: args.next().ok_or(())?,
            },
            Some("remove") => CliHelperAction::SetCodexProtocol {
                action: CodexProtocolAction::Remove,
                result_path: args.next().ok_or(())?,
                rollback: args.next().ok_or(())?,
            },
            _ => return Err(()),
        },
        "--restore-codex-protocol" => CliHelperAction::RestoreCodexProtocol(args.next().ok_or(())?),
        "--install-system" => CliHelperAction::InstallSystem {
            request_path: args.next().ok_or(())?,
            result_path: args.next().ok_or(())?,
        },
        "--update-system" => CliHelperAction::UpdateSystem {
            request_path: args.next().ok_or(())?,
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
        CliHelperAction::Uninstall => Some(
            match (|| -> anyhow::Result<()> {
                let ctx = uninstall::load_context()?;
                let _system_guard = if matches!(ctx.cfg.install_mode, InstallMode::System) {
                    Some(acquire_system_maintenance().map_err(anyhow::Error::msg)?)
                } else {
                    None
                };
                uninstall::run()
            })() {
                Ok(()) => 0,
                Err(cause) => {
                    dialogs::error(&format!("卸载失败：{cause:#}"));
                    1
                }
            },
        ),
        CliHelperAction::UninstallElevated => Some(if uninstall::run_elevated().is_ok() {
            0
        } else {
            1
        }),
        CliHelperAction::DeleteInstalledVersion {
            request_path,
            result_path,
        } => Some(run_system_version_delete_helper(
            Path::new(&request_path),
            Path::new(&result_path),
        )),
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
            action,
            result_path,
            rollback,
        } => Some(run_system_codex_protocol_helper(
            action,
            Path::new(&result_path),
            &rollback,
        )),
        CliHelperAction::RestoreCodexProtocol(encoded) => {
            Some(if restore_system_codex_protocol(&encoded).is_ok() {
                0
            } else {
                1
            })
        }
        CliHelperAction::InstallSystem {
            request_path,
            result_path,
        } => Some(run_system_install_helper(
            Path::new(&request_path),
            Path::new(&result_path),
        )),
        CliHelperAction::UpdateSystem {
            request_path,
            result_path,
        } => Some(run_system_update_helper(
            Path::new(&request_path),
            Path::new(&result_path),
        )),
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
    use super::{
        hex_decode, parse_cli_helper, CliHelperAction, CodexProtocolAction,
        CodexProtocolRollbackPayload,
    };
    use codex_windows_cn::{launcher_update::hex_encode, registry::CodexProtocolBackup};
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Result<Option<CliHelperAction>, ()> {
        parse_cli_helper(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn protocol_action_and_rollback_encoding_match_the_cli_contract() {
        assert_eq!(
            serde_json::from_str::<CodexProtocolAction>(r#""replace""#)
                .expect("deserialize protocol action"),
            CodexProtocolAction::Replace
        );
        let payload = "codex:// 中文回滚".as_bytes();
        assert_eq!(hex_decode(&hex_encode(payload)), Ok(payload.to_vec()));

        let protocol: CodexProtocolBackup = serde_json::from_value(serde_json::json!({
            "snapshot": {
                "root_key_exists": false,
                "icon_key_exists": false,
                "shell_key_exists": false,
                "open_key_exists": false,
                "command_key_exists": false,
                "display_name": null,
                "url_protocol": null,
                "owner": null,
                "install_root": null,
                "icon": null,
                "command": null,
                "delegate_execute": null
            }
        }))
        .expect("deserialize empty protocol backup");
        let install_config =
            b"{\r\n  \"install_mode\": \"system\",\r\n  \"unknown\": true\r\n}\r\n".to_vec();
        let rollback = CodexProtocolRollbackPayload {
            install_root: PathBuf::from(r"C:\Program Files\Codex"),
            protocol,
            install_config: install_config.clone(),
        };
        let encoded = hex_encode(&serde_json::to_vec(&rollback).expect("serialize rollback"));
        let decoded: CodexProtocolRollbackPayload =
            serde_json::from_slice(&hex_decode(&encoded).expect("decode rollback"))
                .expect("deserialize rollback");
        assert_eq!(decoded.install_config, install_config);
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
            parse(&["--set-codex-protocol", "create", "result.json", "00",]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                action: CodexProtocolAction::Create,
                result_path: "result.json".into(),
                rollback: "00".into(),
            }))
        );
        assert_eq!(
            parse(&["--set-codex-protocol", "replace", "result.json", "00",]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                action: CodexProtocolAction::Replace,
                result_path: "result.json".into(),
                rollback: "00".into(),
            }))
        );
        assert_eq!(
            parse(&["--set-codex-protocol", "remove", "result.json", "00",]),
            Ok(Some(CliHelperAction::SetCodexProtocol {
                action: CodexProtocolAction::Remove,
                result_path: "result.json".into(),
                rollback: "00".into(),
            }))
        );
        assert_eq!(
            parse(&["--restore-codex-protocol", "00"]),
            Ok(Some(CliHelperAction::RestoreCodexProtocol("00".into())))
        );
        assert_eq!(
            parse(&["--launch-latest"]),
            Ok(Some(CliHelperAction::LaunchLatest))
        );
    }

    #[test]
    fn parses_system_install_and_update_helpers() {
        assert_eq!(
            parse(&["--delete-installed-version", "request.json", "result.json"]),
            Ok(Some(CliHelperAction::DeleteInstalledVersion {
                request_path: "request.json".into(),
                result_path: "result.json".into(),
            }))
        );
        assert_eq!(
            parse(&["--install-system", "request.json", "result.json"]),
            Ok(Some(CliHelperAction::InstallSystem {
                request_path: "request.json".into(),
                result_path: "result.json".into(),
            }))
        );
        assert_eq!(
            parse(&["--update-system", "request.json", "result.json"]),
            Ok(Some(CliHelperAction::UpdateSystem {
                request_path: "request.json".into(),
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
        assert_eq!(
            parse(&["--uninstall-elevated"]),
            Ok(Some(CliHelperAction::UninstallElevated))
        );
    }

    #[test]
    fn rejects_invalid_helper_arguments() {
        assert_eq!(parse(&["--set-desktop-shortcut"]), Err(()));
        assert_eq!(parse(&["--set-assistant-desktop-shortcut"]), Err(()));
        assert_eq!(parse(&["--set-codex-protocol"]), Err(()));
        assert_eq!(parse(&["--set-codex-protocol", "create"]), Err(()));
        assert_eq!(
            parse(&["--set-codex-protocol", "create", "result.json"]),
            Err(())
        );
        assert_eq!(parse(&["--launch-latest", "extra"]), Err(()));
        assert_eq!(parse(&["--install-system", "request.json"]), Err(()));
        assert_eq!(parse(&["--delete-installed-version"]), Err(()));
        assert_eq!(
            parse(&["--delete-installed-version", "request.json"]),
            Err(())
        );
        assert_eq!(parse(&["--update-system"]), Err(()));
        assert_eq!(parse(&["--update-system", "request.json"]), Err(()));
        assert_eq!(parse(&["--uninstall-system"]), Err(()));
        assert_eq!(parse(&["--uninstall", "extra"]), Err(()));
        assert_eq!(
            parse(&["--update-system", "request.json", "result.json", "extra"]),
            Err(())
        );
    }
}
