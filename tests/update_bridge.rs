use codex_windows_cn::bridge::{
    apply_update_action, update_event_from_msg, update_status_from_decision, UpdateAction,
    UpdateEvent, UpdateEventKind, UpdateStatusKind,
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
