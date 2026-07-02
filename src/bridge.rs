use crate::config::InstallMode;
use crate::installer::{self, InstallMsg, InstallOptions};
use crate::store::Fetcher;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub register_uninstall: bool,
    pub keep_versions: u32,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub mode: BridgeInstallMode,
    pub root: String,
    pub create_shortcut: bool,
    pub register_uninstall: bool,
    pub keep_versions: u32,
    pub fetcher: BridgeFetcher,
    pub use_current_junction: bool,
    pub local_msix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStart {
    pub accepted: bool,
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
        register_uninstall: request.register_uninstall,
        keep_versions: request.keep_versions.max(1),
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
        InstallMsg::Done { version } => InstallEvent {
            kind: InstallEventKind::Done,
            title: "安装完成".into(),
            detail: format!("已安装 Codex {version}"),
            progress: Some(1.0),
            version: Some(version),
            message: None,
        },
        InstallMsg::Error(message) => InstallEvent {
            kind: InstallEventKind::Error,
            title: "安装失败".into(),
            detail: String::new(),
            progress: None,
            version: None,
            message: Some(message),
        },
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
        register_uninstall: user_managed,
        keep_versions: 2,
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

fn phase_title(phase: &str) -> &'static str {
    match phase {
        "Downloading" => "正在下载",
        "Extracting" => "正在解压",
        "Finalizing" => "正在完成",
        _ => "正在安装",
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
