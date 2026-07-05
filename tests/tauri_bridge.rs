use codex_windows_cn::bridge::{app_status, MainPath};

#[test]
fn app_status_describes_the_v1_chinese_install_update_assistant() {
    let status = app_status();

    assert_eq!(status.product_name, "Codex Windows 中文助手");
    assert_eq!(status.v1_boundary, "中文安装更新助手");
    assert_eq!(
        status.main_paths,
        vec![
            MainPath::Install,
            MainPath::ProxyLaunch,
            MainPath::CheckAndUpdate,
            MainPath::Uninstall,
            MainPath::LauncherSelfUpdate,
        ]
    );
}
