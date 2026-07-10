use crate::config::{Config, InstallMode, UpdatePolicy};
use crate::installer::{self, InstallMsg, InstallOptions};
use crate::launcher_update::LauncherUpdateMsg;
use crate::proxy;
use crate::safety;
use crate::store::Fetcher;
use crate::uninstall::UninstallMsg;
use crate::updater::{self, LauncherDecision, UpdateDecision};
use crate::versions::{self, AppKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub product_name: &'static str,
    pub v1_boundary: &'static str,
    pub main_paths: Vec<MainPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MainPath {
    Install,
    ProxyLaunch,
    CheckAndUpdate,
    Uninstall,
    LauncherSelfUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerDefaults {
    pub recommended_mode: BridgeInstallMode,
    pub recommended_fetcher: BridgeFetcher,
    pub modes: Vec<InstallModeDefaults>,
    pub fetchers: Vec<BridgeFetcher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallModeDefaults {
    pub mode: BridgeInstallMode,
    pub label: &'static str,
    pub default_root: String,
    pub create_shortcut: bool,
    pub create_desktop_shortcut: bool,
    pub create_assistant_desktop_shortcut: bool,
    pub register_uninstall: bool,
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub use_current_junction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeInstallMode {
    Portable,
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeFetcher {
    Direct,
    Winget,
    LocalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub mode: BridgeInstallMode,
    pub root: String,
    pub create_shortcut: bool,
    pub create_desktop_shortcut: bool,
    #[serde(default)]
    pub create_assistant_desktop_shortcut: bool,
    pub register_uninstall: bool,
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub fetcher: BridgeFetcher,
    pub use_current_junction: bool,
    pub local_msix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStart {
    pub accepted: bool,
    pub cancellable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallEvent {
    pub kind: InstallEventKind,
    pub title: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallEventKind {
    Phase,
    Progress,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyLaunchStatus {
    pub managed_install: bool,
    pub current_version: Option<String>,
    pub known_latest: Option<String>,
    pub can_launch: bool,
    pub codex_exe: Option<String>,
    pub product_name: String,
    pub running_versions: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyLaunchResult {
    pub launched: bool,
    pub switch_required: bool,
    pub version: Option<String>,
    pub product_name: Option<String>,
    pub running_versions: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    pub version: Option<String>,
    pub switch_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchInstalledRequest {
    pub root: String,
    pub use_current_junction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVersionStatus {
    pub version: String,
    pub app_kind: AppKind,
    pub product_name: String,
    pub executable: String,
    pub size_bytes: u64,
    pub installed_at_unix: u64,
    pub is_default: bool,
    pub is_running: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInventory {
    pub product_name: String,
    pub root: String,
    pub default_version: Option<String>,
    pub running_versions: Vec<String>,
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub update_policy: UpdatePolicy,
    pub fetcher: BridgeFetcher,
    pub use_current_junction: bool,
    pub desktop_shortcut_exists: bool,
    pub assistant_desktop_shortcut_exists: bool,
    pub versions: Vec<InstalledVersionStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSettingsRequest {
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub update_policy: UpdatePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionActionResult {
    pub applied: bool,
    pub message: String,
    pub inventory: VersionInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopShortcutActionResult {
    pub applied: bool,
    pub message: String,
    pub inventory: VersionInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub kind: UpdateStatusKind,
    pub title: String,
    pub message: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub product_name: Option<String>,
    pub actions: Vec<UpdateAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStart {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActionResult {
    pub applied: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateStatus {
    pub kind: LauncherUpdateStatusKind,
    pub title: String,
    pub message: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub actions: Vec<LauncherUpdateAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherUpdateStatusKind {
    UpToDate,
    Available,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherUpdateAction {
    UpdateNow,
    ViewRelease,
    NotNow,
    SkipThisVersion,
    SnoozeOneDay,
    SnoozeSevenDays,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateActionResult {
    pub applied: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateStart {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateEvent {
    pub kind: LauncherUpdateEventKind,
    pub title: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherUpdateEventKind {
    Phase,
    Progress,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallConfirmation {
    pub title: String,
    pub root: String,
    pub delete_items: Vec<String>,
    pub preserve_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallStatus {
    pub kind: UninstallStatusKind,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallStatusKind {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallStart {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallEvent {
    pub kind: UninstallEventKind,
    pub title: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub log_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallEventKind {
    Phase,
    Progress,
    Done,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatusKind {
    UpToDate,
    Available,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateAction {
    UpdateNow,
    NotNow,
    SkipThisVersion,
    SnoozeOneDay,
    SnoozeSevenDays,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEvent {
    pub kind: UpdateEventKind,
    pub title: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateEventKind {
    Phase,
    Progress,
    Done,
    Error,
}

pub fn app_status() -> AppStatus {
    AppStatus {
        product_name: "Codex Windows 中文助手",
        v1_boundary: "中文安装更新助手",
        main_paths: vec![
            MainPath::Install,
            MainPath::ProxyLaunch,
            MainPath::CheckAndUpdate,
            MainPath::Uninstall,
            MainPath::LauncherSelfUpdate,
        ],
    }
}

pub fn update_status_from_decision(decision: UpdateDecision) -> UpdateStatus {
    match decision {
        UpdateDecision::Available {
            current,
            latest,
            product_name,
        } => UpdateStatus {
            kind: UpdateStatusKind::Available,
            title: format!("发现 {product_name} 新版本"),
            message: format!("当前版本 {current}，可更新到 {latest}"),
            current_version: Some(current),
            latest_version: Some(latest),
            product_name: Some(product_name),
            actions: vec![
                UpdateAction::UpdateNow,
                UpdateAction::NotNow,
                UpdateAction::SkipThisVersion,
                UpdateAction::SnoozeOneDay,
                UpdateAction::SnoozeSevenDays,
                UpdateAction::Never,
            ],
        },
        UpdateDecision::UpToDate {
            version,
            product_name,
        } => UpdateStatus {
            kind: UpdateStatusKind::UpToDate,
            title: format!("{product_name} 已是最新版本"),
            message: format!("当前版本 {version} 已是最新"),
            current_version: Some(version.clone()),
            latest_version: Some(version),
            product_name: Some(product_name),
            actions: Vec::new(),
        },
        UpdateDecision::Skipped { reason } => UpdateStatus {
            kind: UpdateStatusKind::Skipped,
            title: "暂不检查更新".into(),
            message: reason,
            current_version: None,
            latest_version: None,
            product_name: None,
            actions: Vec::new(),
        },
        UpdateDecision::Error(message) => UpdateStatus {
            kind: UpdateStatusKind::Error,
            title: "检查更新失败".into(),
            message,
            current_version: None,
            latest_version: None,
            product_name: None,
            actions: Vec::new(),
        },
    }
}

pub fn launcher_update_status_from_decision(decision: LauncherDecision) -> LauncherUpdateStatus {
    match decision {
        LauncherDecision::Available {
            current,
            latest,
            release_url,
        } => LauncherUpdateStatus {
            kind: LauncherUpdateStatusKind::Available,
            title: "发现启动器新版本".into(),
            message: format!("当前版本 {current}，可更新到 {latest}"),
            current_version: Some(current),
            latest_version: Some(latest),
            release_url: Some(release_url),
            actions: vec![
                LauncherUpdateAction::UpdateNow,
                LauncherUpdateAction::ViewRelease,
                LauncherUpdateAction::NotNow,
                LauncherUpdateAction::SkipThisVersion,
                LauncherUpdateAction::SnoozeOneDay,
                LauncherUpdateAction::SnoozeSevenDays,
                LauncherUpdateAction::Never,
            ],
        },
        LauncherDecision::UpToDate { version } => LauncherUpdateStatus {
            kind: LauncherUpdateStatusKind::UpToDate,
            title: "启动器已是最新".into(),
            message: format!("当前启动器版本 {version}"),
            current_version: Some(version.clone()),
            latest_version: Some(version),
            release_url: None,
            actions: Vec::new(),
        },
        LauncherDecision::Skipped { reason } => LauncherUpdateStatus {
            kind: LauncherUpdateStatusKind::Skipped,
            title: "暂不检查启动器更新".into(),
            message: reason,
            current_version: None,
            latest_version: None,
            release_url: None,
            actions: Vec::new(),
        },
        LauncherDecision::Error(message) => LauncherUpdateStatus {
            kind: LauncherUpdateStatusKind::Error,
            title: launcher_error_title(&message).into(),
            message: launcher_error_message(&message),
            current_version: None,
            latest_version: None,
            release_url: None,
            actions: Vec::new(),
        },
    }
}

pub fn uninstall_confirmation(root: &Path) -> UninstallConfirmation {
    UninstallConfirmation {
        title: "确认卸载 Codex Windows 中文助手".into(),
        root: root.to_string_lossy().into_owned(),
        delete_items: vec![
            "已安装的桌面应用版本".into(),
            "下载缓存".into(),
            "启动器配置".into(),
            "开始菜单和桌面快捷方式".into(),
            "Windows 卸载入口".into(),
        ],
        preserve_items: vec!["Codex/ChatGPT 登录数据".into(), "日志和诊断信息".into()],
    }
}

pub fn uninstall_status_for_root(root: &Path) -> UninstallStatus {
    match safety::validate_uninstall_root(root) {
        Ok(()) => UninstallStatus {
            kind: UninstallStatusKind::Ready,
            title: "可以卸载".into(),
            message: "将只删除启动器管理的文件".into(),
        },
        Err(cause) => UninstallStatus {
            kind: UninstallStatusKind::Blocked,
            title: "无法卸载".into(),
            message: format!("拒绝卸载：{cause}"),
        },
    }
}

pub fn uninstall_event_from_msg(msg: UninstallMsg) -> UninstallEvent {
    match msg {
        UninstallMsg::Phase { phase, detail } => UninstallEvent {
            kind: UninstallEventKind::Phase,
            title: uninstall_phase_title(&phase).into(),
            detail,
            progress: None,
            log_path: None,
            message: None,
        },
        UninstallMsg::Progress(progress) => UninstallEvent {
            kind: UninstallEventKind::Progress,
            title: "卸载进度".into(),
            detail: String::new(),
            progress,
            log_path: None,
            message: None,
        },
        UninstallMsg::Done { log_path } => UninstallEvent {
            kind: UninstallEventKind::Done,
            title: "卸载完成".into(),
            detail: if log_path.is_empty() {
                "卸载已完成".into()
            } else {
                format!("卸载日志：{log_path}")
            },
            progress: Some(1.0),
            log_path: if log_path.is_empty() {
                None
            } else {
                Some(log_path)
            },
            message: None,
        },
        UninstallMsg::Error(message) => UninstallEvent {
            kind: UninstallEventKind::Error,
            title: "卸载失败".into(),
            detail: String::new(),
            progress: None,
            log_path: None,
            message: Some(message),
        },
    }
}

pub fn apply_update_action(cfg: &mut Config, action: UpdateAction, latest: &str) {
    updater::apply_defer(cfg, defer_choice(action), latest);
}

pub fn apply_launcher_update_action(cfg: &mut Config, action: LauncherUpdateAction, latest: &str) {
    updater::apply_launcher_defer(cfg, launcher_defer_choice(action), latest);
}

pub fn update_event_from_msg(msg: InstallMsg) -> UpdateEvent {
    match msg {
        InstallMsg::Phase { phase, detail } => UpdateEvent {
            kind: UpdateEventKind::Phase,
            title: update_phase_title(&phase).into(),
            detail: phase_detail(&detail),
            progress: None,
            version: None,
            message: None,
        },
        InstallMsg::Progress(progress) => UpdateEvent {
            kind: UpdateEventKind::Progress,
            title: "更新进度".into(),
            detail: String::new(),
            progress,
            version: None,
            message: None,
        },
        InstallMsg::ProgressDetail {
            phase,
            detail,
            progress,
        } => UpdateEvent {
            kind: UpdateEventKind::Progress,
            title: update_phase_title(&phase).into(),
            detail: phase_detail(&detail),
            progress,
            version: None,
            message: None,
        },
        InstallMsg::Done { version } => UpdateEvent {
            kind: UpdateEventKind::Done,
            title: "更新完成".into(),
            detail: format!("已更新官方桌面应用到 {version}"),
            progress: Some(1.0),
            version: Some(version),
            message: None,
        },
        InstallMsg::Error(message) => {
            let cancelled = message == "安装已取消";
            UpdateEvent {
                kind: UpdateEventKind::Error,
                title: if cancelled {
                    "更新已取消"
                } else {
                    "更新失败"
                }
                .into(),
                detail: String::new(),
                progress: None,
                version: None,
                message: Some(message),
            }
        }
    }
}

pub fn launcher_update_event_from_msg(msg: LauncherUpdateMsg) -> LauncherUpdateEvent {
    match msg {
        LauncherUpdateMsg::Phase { phase, detail } => LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Phase,
            title: launcher_update_phase_title(&phase).into(),
            detail,
            progress: None,
            message: None,
        },
        LauncherUpdateMsg::Progress(progress) => LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Progress,
            title: "自更新进度".into(),
            detail: String::new(),
            progress,
            message: None,
        },
        LauncherUpdateMsg::Done => LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Done,
            title: "自更新完成".into(),
            detail: "启动器已更新，重启后生效".into(),
            progress: Some(1.0),
            message: None,
        },
        LauncherUpdateMsg::Error(message) => LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Error,
            title: "自更新失败".into(),
            detail: String::new(),
            progress: None,
            message: Some(message),
        },
    }
}

pub fn proxy_launch_status(root: &Path, cfg: &Config) -> ProxyLaunchStatus {
    match version_inventory(root, cfg) {
        Ok(inventory) if !inventory.versions.is_empty() => {
            let exe = proxy::resolve_codex_exe(root, cfg.use_current_junction);
            ProxyLaunchStatus {
                managed_install: true,
                current_version: inventory.default_version,
                known_latest: cfg.known_latest.clone(),
                can_launch: exe.is_some(),
                codex_exe: exe.map(|path| path.to_string_lossy().into_owned()),
                product_name: inventory.product_name.clone(),
                running_versions: inventory.running_versions,
                message: format!("可以启动 {}", inventory.product_name),
            }
        }
        _ => ProxyLaunchStatus {
            managed_install: true,
            current_version: Some(cfg.current_version.clone()),
            known_latest: cfg.known_latest.clone(),
            can_launch: false,
            codex_exe: None,
            product_name: "Codex".into(),
            running_versions: Vec::new(),
            message: "未找到可启动的 Codex 或 ChatGPT".into(),
        },
    }
}

pub fn version_inventory(root: &Path, cfg: &Config) -> anyhow::Result<VersionInventory> {
    let installed = versions::scan_installed(root)?;
    let running = proxy::running_versions(&root.join("versions"));
    let default_version = installed.first().map(|item| item.version.clone());
    let product_name = installed
        .first()
        .map(|item| item.app_kind.display_name())
        .unwrap_or("Codex")
        .to_string();
    let can_remove = installed.len() > 1;
    let mut running_versions: Vec<String> = running.iter().cloned().collect();
    running_versions.sort();
    let versions = installed
        .into_iter()
        .map(|item| {
            let is_default = default_version.as_deref() == Some(item.version.as_str());
            let is_running = running.contains(&item.version);
            InstalledVersionStatus {
                version: item.version,
                app_kind: item.app_kind,
                product_name: item.app_kind.display_name().to_string(),
                executable: item.executable.to_string_lossy().into_owned(),
                size_bytes: item.size_bytes,
                installed_at_unix: item.installed_at_unix,
                is_default,
                is_running,
                can_delete: can_remove && !is_running,
            }
        })
        .collect();

    Ok(VersionInventory {
        product_name,
        root: root.to_string_lossy().into_owned(),
        default_version,
        running_versions,
        keep_versions: cfg.keep_versions.max(1),
        keep_all_versions: cfg.keep_all_versions,
        update_policy: cfg.update_policy,
        fetcher: bridge_fetcher(cfg.fetcher),
        use_current_junction: cfg.use_current_junction,
        desktop_shortcut_exists: crate::installer::desktop_shortcut_exists(root, cfg.install_mode)?,
        assistant_desktop_shortcut_exists: crate::installer::assistant_desktop_shortcut_exists(
            root,
            cfg.install_mode,
        )?,
        versions,
    })
}

pub fn persist_version_settings(
    root: &Path,
    cfg: &mut Config,
    request: VersionSettingsRequest,
) -> anyhow::Result<VersionActionResult> {
    if matches!(cfg.update_policy, UpdatePolicy::Never)
        && !matches!(request.update_policy, UpdatePolicy::Never)
    {
        cfg.suppress_until_unix = None;
        cfg.launcher_suppress_until_unix = None;
    }
    cfg.keep_versions = request.keep_versions.max(1);
    cfg.keep_all_versions = request.keep_all_versions;
    cfg.update_policy = request.update_policy;
    cfg.save_runtime(root)?;
    let inventory = version_inventory(root, cfg)?;
    Ok(VersionActionResult {
        applied: true,
        message: "版本策略已保存".into(),
        inventory,
    })
}

pub fn launch_result_from_outcome(outcome: proxy::LaunchOutcome) -> ProxyLaunchResult {
    match outcome {
        proxy::LaunchOutcome::Launched { version, app_kind } => ProxyLaunchResult {
            launched: true,
            switch_required: false,
            version: Some(version.clone()),
            product_name: Some(app_kind.display_name().to_string()),
            running_versions: Vec::new(),
            message: format!("已启动 {} {version}", app_kind.display_name()),
        },
        proxy::LaunchOutcome::SwitchRequired {
            running_versions,
            target_version,
        } => ProxyLaunchResult {
            launched: false,
            switch_required: true,
            version: Some(target_version.clone()),
            product_name: None,
            running_versions,
            message: format!("需要关闭当前版本后切换到 {target_version}"),
        },
    }
}

pub fn install_options_from_request(request: InstallRequest) -> Result<InstallOptions, String> {
    let root = request.root.trim();
    if root.is_empty() {
        return Err("请选择安装位置".into());
    }

    let fetcher = core_fetcher(request.fetcher);
    let local_msix = request
        .local_msix
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from);
    if matches!(fetcher, Fetcher::LocalFile) && local_msix.is_none() {
        return Err("使用本地 MSIX 时需要选择文件".into());
    }

    Ok(InstallOptions {
        mode: core_install_mode(request.mode),
        root: PathBuf::from(root),
        create_shortcut: request.create_shortcut,
        create_desktop_shortcut: request.create_desktop_shortcut,
        create_assistant_desktop_shortcut: request.create_assistant_desktop_shortcut,
        register_uninstall: request.register_uninstall,
        keep_versions: request.keep_versions.max(1),
        keep_all_versions: request.keep_all_versions,
        fetcher,
        use_current_junction: request.use_current_junction,
        local_msix,
    })
}

pub fn install_event_from_msg(msg: InstallMsg) -> InstallEvent {
    match msg {
        InstallMsg::Phase { phase, detail } => InstallEvent {
            kind: InstallEventKind::Phase,
            title: phase_title(&phase).into(),
            detail: phase_detail(&detail),
            progress: None,
            version: None,
            message: None,
        },
        InstallMsg::Progress(progress) => InstallEvent {
            kind: InstallEventKind::Progress,
            title: "安装进度".into(),
            detail: String::new(),
            progress,
            version: None,
            message: None,
        },
        InstallMsg::ProgressDetail {
            phase,
            detail,
            progress,
        } => InstallEvent {
            kind: InstallEventKind::Progress,
            title: phase_title(&phase).into(),
            detail: phase_detail(&detail),
            progress,
            version: None,
            message: None,
        },
        InstallMsg::Done { version } => InstallEvent {
            kind: InstallEventKind::Done,
            title: "安装完成".into(),
            detail: format!("已安装官方桌面应用 {version}"),
            progress: Some(1.0),
            version: Some(version),
            message: None,
        },
        InstallMsg::Error(message) => {
            let cancelled = message == "安装已取消";
            InstallEvent {
                kind: InstallEventKind::Error,
                title: if cancelled {
                    "安装已取消"
                } else {
                    "安装失败"
                }
                .into(),
                detail: String::new(),
                progress: None,
                version: None,
                message: Some(message),
            }
        }
    }
}

pub fn installer_defaults() -> InstallerDefaults {
    InstallerDefaults {
        recommended_mode: BridgeInstallMode::User,
        recommended_fetcher: bridge_fetcher(Fetcher::Direct),
        modes: vec![
            install_mode_defaults(InstallMode::Portable),
            install_mode_defaults(InstallMode::User),
            install_mode_defaults(InstallMode::System),
        ],
        fetchers: [Fetcher::Direct, Fetcher::Winget, Fetcher::LocalFile]
            .into_iter()
            .map(bridge_fetcher)
            .collect(),
    }
}

fn install_mode_defaults(mode: InstallMode) -> InstallModeDefaults {
    let user_managed = !matches!(mode, InstallMode::Portable);

    InstallModeDefaults {
        mode: bridge_install_mode(mode),
        label: install_mode_label(mode),
        default_root: installer::default_path(mode).to_string_lossy().into_owned(),
        create_shortcut: user_managed,
        create_desktop_shortcut: user_managed,
        create_assistant_desktop_shortcut: false,
        register_uninstall: user_managed,
        keep_versions: 5,
        keep_all_versions: false,
        use_current_junction: true,
    }
}

fn install_mode_label(mode: InstallMode) -> &'static str {
    match mode {
        InstallMode::Portable => "便携模式",
        InstallMode::User => "当前用户",
        InstallMode::System => "所有用户",
    }
}

fn bridge_install_mode(mode: InstallMode) -> BridgeInstallMode {
    match mode {
        InstallMode::Portable => BridgeInstallMode::Portable,
        InstallMode::User => BridgeInstallMode::User,
        InstallMode::System => BridgeInstallMode::System,
    }
}

fn bridge_fetcher(fetcher: Fetcher) -> BridgeFetcher {
    match fetcher {
        Fetcher::Direct => BridgeFetcher::Direct,
        Fetcher::Winget => BridgeFetcher::Winget,
        Fetcher::LocalFile => BridgeFetcher::LocalFile,
    }
}

fn core_install_mode(mode: BridgeInstallMode) -> InstallMode {
    match mode {
        BridgeInstallMode::Portable => InstallMode::Portable,
        BridgeInstallMode::User => InstallMode::User,
        BridgeInstallMode::System => InstallMode::System,
    }
}

fn core_fetcher(fetcher: BridgeFetcher) -> Fetcher {
    match fetcher {
        BridgeFetcher::Direct => Fetcher::Direct,
        BridgeFetcher::Winget => Fetcher::Winget,
        BridgeFetcher::LocalFile => Fetcher::LocalFile,
    }
}

fn defer_choice(action: UpdateAction) -> updater::DeferChoice {
    match action {
        UpdateAction::UpdateNow => updater::DeferChoice::UpdateNow,
        UpdateAction::NotNow => updater::DeferChoice::NotNow,
        UpdateAction::SkipThisVersion => updater::DeferChoice::SkipThisVersion,
        UpdateAction::SnoozeOneDay => updater::DeferChoice::SnoozeOneDay,
        UpdateAction::SnoozeSevenDays => updater::DeferChoice::SnoozeSevenDays,
        UpdateAction::Never => updater::DeferChoice::Never,
    }
}

fn launcher_defer_choice(action: LauncherUpdateAction) -> updater::LauncherDeferChoice {
    match action {
        LauncherUpdateAction::UpdateNow => updater::LauncherDeferChoice::ApplyUpdate,
        LauncherUpdateAction::ViewRelease => updater::LauncherDeferChoice::ViewRelease,
        LauncherUpdateAction::NotNow => updater::LauncherDeferChoice::NotNow,
        LauncherUpdateAction::SkipThisVersion => updater::LauncherDeferChoice::SkipThisVersion,
        LauncherUpdateAction::SnoozeOneDay => updater::LauncherDeferChoice::SnoozeOneDay,
        LauncherUpdateAction::SnoozeSevenDays => updater::LauncherDeferChoice::SnoozeSevenDays,
        LauncherUpdateAction::Never => updater::LauncherDeferChoice::Never,
    }
}

fn phase_title(phase: &str) -> &'static str {
    match phase {
        "Elevating" => "正在请求管理员权限",
        "Downloading" => "正在下载",
        "Extracting" => "正在解压",
        "Finalizing" => "正在完成",
        _ => "正在安装",
    }
}

fn update_phase_title(phase: &str) -> &'static str {
    match phase {
        "Elevating" => "正在请求管理员权限",
        "Downloading" => "正在下载更新",
        "Extracting" => "正在解压更新",
        "Finalizing" => "正在完成更新",
        _ => "正在更新",
    }
}

fn launcher_update_phase_title(phase: &str) -> &'static str {
    match phase {
        "Downloading launcher" => "正在下载启动器",
        "Verifying" => "正在校验 SHA-256",
        "Smoke-testing" => "正在运行自检",
        "Installing" => "正在替换启动器",
        _ => "正在自更新",
    }
}

fn launcher_error_title(message: &str) -> &'static str {
    if is_github_rate_limit_error(message) {
        return "检查启动器更新受限";
    }
    "检查启动器更新失败"
}

fn launcher_error_message(message: &str) -> String {
    if is_github_rate_limit_error(message) {
        return "GitHub 暂时限制了未登录接口请求，稍后会自动重试。".into();
    }
    message.into()
}

fn is_github_rate_limit_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("403") && lower.contains("rate limit")
}

fn uninstall_phase_title(phase: &str) -> &'static str {
    match phase {
        "Elevating" => "正在请求管理员权限",
        "Validating install" => "正在校验安装目录",
        "Terminating Codex" => "正在结束 Codex 进程",
        "Removing shortcuts" => "正在移除开始菜单和桌面快捷方式",
        "Removing registry entries" => "正在移除 Windows 卸载入口",
        "Removing versions/current junction" => "正在移除 current 入口",
        "Deleting files" => "正在删除文件",
        "Deleting versioned installs" => "正在删除已安装版本",
        "Deleting download cache" => "正在删除下载缓存",
        "Removing config" => "正在移除启动器配置",
        "Finalizing" => "正在完成卸载",
        _ => "正在卸载",
    }
}

fn phase_detail(detail: &str) -> String {
    match detail {
        "via Direct" => "通过直连 Microsoft Store".into(),
        "via Winget" => "通过 winget".into(),
        "via LocalFile" => "通过本地 MSIX".into(),
        _ if detail.starts_with("version ") => {
            format!("版本 {}", detail.trim_start_matches("version "))
        }
        _ => detail.into(),
    }
}
