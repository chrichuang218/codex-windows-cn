use codex_windows_cn::bridge::{
    apply_update_action, install_event_from_msg, update_event_from_msg,
    update_status_from_decision, UpdateAction, UpdateEvent, UpdateEventKind, UpdateStatusKind,
};
use codex_windows_cn::config::{Config, InstallMode, UpdatePolicy};
use codex_windows_cn::installer::InstallMsg;
use codex_windows_cn::store::Fetcher;
use codex_windows_cn::updater::UpdateDecision;

#[test]
fn update_available_decision_becomes_chinese_status_with_user_actions() {
    let status = update_status_from_decision(UpdateDecision::Available {
        current: "1.0.0".into(),
        latest: "1.2.0".into(),
        product_name: "ChatGPT".into(),
    });

    assert_eq!(status.kind, UpdateStatusKind::Available);
    assert_eq!(status.title, "发现 ChatGPT 新版本");
    assert_eq!(status.message, "当前版本 1.0.0，可更新到 1.2.0");
    assert_eq!(status.current_version.as_deref(), Some("1.0.0"));
    assert_eq!(status.latest_version.as_deref(), Some("1.2.0"));
    assert_eq!(status.product_name.as_deref(), Some("ChatGPT"));
    assert_eq!(
        status.actions,
        vec![
            UpdateAction::UpdateNow,
            UpdateAction::NotNow,
            UpdateAction::SkipThisVersion,
            UpdateAction::SnoozeOneDay,
            UpdateAction::SnoozeSevenDays,
            UpdateAction::Never,
        ]
    );
}

#[test]
fn update_error_decision_becomes_non_blocking_chinese_status() {
    let status = update_status_from_decision(UpdateDecision::Error("network failed".into()));

    assert_eq!(status.kind, UpdateStatusKind::Error);
    assert_eq!(status.title, "检查更新失败");
    assert_eq!(status.message, "network failed");
    assert!(status.actions.is_empty());
}

#[test]
fn update_worker_messages_are_reported_as_chinese_events() {
    let event = update_event_from_msg(InstallMsg::Done {
        version: "1.2.0".into(),
    });

    assert_eq!(
        event,
        UpdateEvent {
            kind: UpdateEventKind::Done,
            title: "更新完成".into(),
            detail: "已更新官方桌面应用到 1.2.0".into(),
            progress: Some(1.0),
            version: Some("1.2.0".into()),
            message: None,
        }
    );
}

#[test]
fn download_failures_show_chinese_reasons_and_preserve_details_for_install_and_update() {
    let cases = [
        (
            "读取安装包失败（已下载 7 字节）: request or response body error: error reading a body from connection: end of file before message length reached",
            "下载过程中网络连接中断，请检查网络后重试。",
        ),
        (
            "FE3 GetCookie failed: error decoding response body: error reading a body from connection: end of file before message length reached",
            "下载过程中网络连接中断，请检查网络后重试。",
        ),
        (
            "error sending request for url (https://example.invalid): client error (SendRequest): connection closed before message completed",
            "下载过程中网络连接中断，请检查网络后重试。",
        ),
        (
            "FE3 GetCookie failed: error sending request for url (https://example.invalid): operation timed out",
            "下载超时，请检查网络后重试。",
        ),
        (
            "error sending request for url (https://example.invalid): client error (Connect): dns error",
            "无法连接下载服务器，请检查网络后重试。",
        ),
        (
            "error sending request for url (https://example.invalid): invalid peer certificate: UnknownIssuer",
            "下载连接的证书验证失败，请检查网络环境后重试。",
        ),
        (
            "FE3 SyncUpdates failed: HTTP status server error (503 Service Unavailable)",
            "下载服务器返回错误，请稍后重试。",
        ),
        (
            "winget download failed (exit Some(-1978335114))\n--- stdout ---\nAuthentication required",
            "winget 下载失败，请查看错误详情后重试。",
        ),
    ];
    for (raw, summary) in cases {
        let install = install_event_from_msg(InstallMsg::Error(raw.into()));
        let update = update_event_from_msg(InstallMsg::Error(raw.into()));
        assert_eq!(install.message.as_deref(), Some(summary), "install: {raw}");
        assert_eq!(update.message.as_deref(), Some(summary), "update: {raw}");
        assert_eq!(install.detail, raw);
        assert_eq!(update.detail, raw);
    }
}

#[test]
fn non_network_failures_keep_their_original_reason() {
    for raw in [
        "安装已取消",
        "写入安装包失败：磁盘空间不足",
        "creating install root: Access is denied.",
        "DisplayCatalog lookup failed: error decoding response body: expected value at line 1 column 1",
    ] {
        let install = install_event_from_msg(InstallMsg::Error(raw.into()));
        let update = update_event_from_msg(InstallMsg::Error(raw.into()));
        assert_eq!(install.message.as_deref(), Some(raw));
        assert_eq!(update.message.as_deref(), Some(raw));
    }
}

#[test]
fn update_defer_action_preserves_existing_config_semantics() {
    let mut cfg = test_config();

    apply_update_action(&mut cfg, UpdateAction::SkipThisVersion, "1.2.0");

    assert_eq!(cfg.skipped_version.as_deref(), Some("1.2.0"));
    assert_eq!(cfg.known_latest.as_deref(), Some("1.2.0"));
    assert_eq!(cfg.update_policy, UpdatePolicy::Daily);
}

fn test_config() -> Config {
    Config {
        install_mode: InstallMode::User,
        current_version: "1.0.0".into(),
        update_policy: UpdatePolicy::Daily,
        last_check_unix: None,
        last_launcher_check_unix: None,
        suppress_until_unix: None,
        known_latest: None,
        skipped_version: None,
        keep_versions: 2,
        keep_all_versions: false,
        fetcher: Fetcher::Direct,
        use_current_junction: true,
        register_uninstall: true,
        known_latest_launcher: None,
        skipped_launcher_version: None,
        launcher_suppress_until_unix: None,
    }
}
