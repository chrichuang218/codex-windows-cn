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

#[test]
fn html_preboot_uses_the_same_assistant_brand() {
    let html = std::fs::read_to_string("frontend/index.html").expect("frontend entry should exist");

    assert!(html.contains("Codex Windows 中文助手"));
    assert!(html.contains("中文安装更新助手"));
    assert!(html.contains("preboot-mark"));
    assert!(
        !html.contains("Codex Updater"),
        "preboot should not flash the old updater brand"
    );
}
