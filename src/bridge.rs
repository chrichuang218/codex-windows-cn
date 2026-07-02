use serde::Serialize;

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
