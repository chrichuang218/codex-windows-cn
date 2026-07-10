use codex_windows_cn::bridge::{
    install_event_from_msg, install_options_from_request, installer_defaults, BridgeFetcher,
    BridgeInstallMode, InstallEvent, InstallEventKind, InstallRequest,
};
use codex_windows_cn::installer::InstallMsg;
use codex_windows_cn::{config::InstallMode, store::Fetcher};
use std::path::PathBuf;

#[test]
fn installer_defaults_offer_a_recommended_user_install_path() {
    let defaults = installer_defaults();

    assert_eq!(defaults.recommended_mode, BridgeInstallMode::User);
    assert_eq!(defaults.recommended_fetcher, BridgeFetcher::Direct);
    assert_eq!(
        defaults
            .modes
            .iter()
            .map(|mode| mode.mode)
            .collect::<Vec<_>>(),
        vec![
            BridgeInstallMode::Portable,
            BridgeInstallMode::User,
            BridgeInstallMode::System,
        ]
    );
    assert_eq!(
        defaults.fetchers,
        vec![
            BridgeFetcher::Direct,
            BridgeFetcher::Winget,
            BridgeFetcher::LocalFile,
        ]
    );

    let user_mode = defaults
        .modes
        .iter()
        .find(|mode| mode.mode == BridgeInstallMode::User)
        .expect("user install mode should be available");
    assert_eq!(user_mode.label, "当前用户");
    assert!(user_mode.default_root.ends_with(r"\Codex"));
    assert!(user_mode.create_shortcut);
    assert!(user_mode.create_desktop_shortcut);
    assert!(!user_mode.create_assistant_desktop_shortcut);
    assert!(user_mode.register_uninstall);
    assert_eq!(user_mode.keep_versions, 5);
    assert!(!user_mode.keep_all_versions);
    assert!(user_mode.use_current_junction);

    let portable_mode = defaults
        .modes
        .iter()
        .find(|mode| mode.mode == BridgeInstallMode::Portable)
        .expect("portable install mode should be available");
    assert_eq!(portable_mode.label, "便携模式");
    assert!(!portable_mode.create_shortcut);
    assert!(!portable_mode.create_desktop_shortcut);
    assert!(!portable_mode.create_assistant_desktop_shortcut);
    assert!(!portable_mode.register_uninstall);
}

#[test]
fn install_request_builds_installer_options_without_touching_disk() {
    let request = InstallRequest {
        mode: BridgeInstallMode::User,
        root: r"C:\Users\tester\AppData\Local\Codex".into(),
        create_shortcut: true,
        create_desktop_shortcut: true,
        create_assistant_desktop_shortcut: true,
        register_uninstall: true,
        keep_versions: 2,
        keep_all_versions: false,
        fetcher: BridgeFetcher::Direct,
        use_current_junction: true,
        local_msix: None,
    };

    let options = install_options_from_request(request).expect("valid install request");

    assert_eq!(options.mode, InstallMode::User);
    assert_eq!(
        options.root,
        PathBuf::from(r"C:\Users\tester\AppData\Local\Codex")
    );
    assert!(options.create_shortcut);
    assert!(options.create_desktop_shortcut);
    assert!(options.create_assistant_desktop_shortcut);
    assert!(options.register_uninstall);
    assert_eq!(options.keep_versions, 2);
    assert_eq!(options.fetcher, Fetcher::Direct);
    assert!(options.use_current_junction);
    assert!(options.local_msix.is_none());
}

#[test]
fn install_worker_messages_are_reported_as_chinese_events() {
    let event = install_event_from_msg(InstallMsg::Phase {
        phase: "Downloading".into(),
        detail: "via Direct".into(),
    });

    assert_eq!(
        event,
        InstallEvent {
            kind: InstallEventKind::Phase,
            title: "正在下载".into(),
            detail: "通过直连 Microsoft Store".into(),
            progress: None,
            version: None,
            message: None,
        }
    );
}

#[test]
fn install_done_does_not_assume_the_downloaded_product_name() {
    let event = install_event_from_msg(InstallMsg::Done {
        version: "26.707.3748.0".into(),
    });

    assert_eq!(event.title, "安装完成");
    assert_eq!(event.detail, "已安装官方桌面应用 26.707.3748.0");
}

#[test]
fn install_progress_event_keeps_download_title_and_size_detail() {
    let event = install_event_from_msg(InstallMsg::ProgressDetail {
        phase: "Downloading".into(),
        detail: "538 / 639 MB".into(),
        progress: Some(0.842),
    });

    assert_eq!(
        event,
        InstallEvent {
            kind: InstallEventKind::Progress,
            title: "正在下载".into(),
            detail: "538 / 639 MB".into(),
            progress: Some(0.842),
            version: None,
            message: None,
        }
    );
}

#[test]
fn install_progress_event_keeps_extract_title_and_file_count() {
    let event = install_event_from_msg(InstallMsg::ProgressDetail {
        phase: "Extracting".into(),
        detail: "318 / 900 files".into(),
        progress: Some(0.353),
    });

    assert_eq!(
        event,
        InstallEvent {
            kind: InstallEventKind::Progress,
            title: "正在解压".into(),
            detail: "318 / 900 files".into(),
            progress: Some(0.353),
            version: None,
            message: None,
        }
    );
}

#[test]
fn install_progress_event_keeps_finalizing_title_and_detail() {
    let event = install_event_from_msg(InstallMsg::ProgressDetail {
        phase: "Finalizing".into(),
        detail: "正在写入启动器和配置。".into(),
        progress: None,
    });

    assert_eq!(
        event,
        InstallEvent {
            kind: InstallEventKind::Progress,
            title: "正在完成".into(),
            detail: "正在写入启动器和配置。".into(),
            progress: None,
            version: None,
            message: None,
        }
    );
}
