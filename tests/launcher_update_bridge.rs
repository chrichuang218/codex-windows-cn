use codex_windows_cn::bridge::{
    apply_launcher_update_action, launcher_update_event_from_msg,
    launcher_update_status_from_decision, LauncherUpdateAction, LauncherUpdateEvent,
    LauncherUpdateEventKind, LauncherUpdateStatusKind,
};
use codex_windows_cn::config::{Config, InstallMode, UpdatePolicy};
use codex_windows_cn::launcher_update::LauncherUpdateMsg;
use codex_windows_cn::store::Fetcher;
use codex_windows_cn::updater::{LauncherDecision, LAUNCHER_LATEST_API, LAUNCHER_REPO};

#[test]
fn launcher_release_source_uses_this_project_repository() {
    assert_eq!(LAUNCHER_REPO, "chrichuang218/codex-windows-cn");
    assert_eq!(
        LAUNCHER_LATEST_API,
        "https://api.github.com/repos/chrichuang218/codex-windows-cn/releases/latest"
    );
}

#[test]
fn launcher_update_available_decision_becomes_chinese_status_with_actions() {
    let status = launcher_update_status_from_decision(LauncherDecision::Available {
        current: "0.1.2".into(),
        latest: "0.2.0".into(),
        release_url: "https://github.com/chrichuang218/codex-windows-cn/releases/tag/v0.2.0".into(),
    });

    assert_eq!(status.kind, LauncherUpdateStatusKind::Available);
    assert_eq!(status.title, "发现启动器新版本");
    assert_eq!(status.message, "当前版本 0.1.2，可更新到 0.2.0");
    assert_eq!(status.current_version.as_deref(), Some("0.1.2"));
    assert_eq!(status.latest_version.as_deref(), Some("0.2.0"));
    assert_eq!(
        status.release_url.as_deref(),
        Some("https://github.com/chrichuang218/codex-windows-cn/releases/tag/v0.2.0")
    );
    assert_eq!(
        status.actions,
        vec![
            LauncherUpdateAction::UpdateNow,
            LauncherUpdateAction::ViewRelease,
            LauncherUpdateAction::NotNow,
            LauncherUpdateAction::SkipThisVersion,
            LauncherUpdateAction::SnoozeOneDay,
            LauncherUpdateAction::SnoozeSevenDays,
            LauncherUpdateAction::Never,
        ]
    );
}

#[test]
fn launcher_update_worker_messages_are_reported_as_chinese_events() {
    let event = launcher_update_event_from_msg(LauncherUpdateMsg::Phase {
        phase: "Verifying".into(),
        detail: "checking SHA-256".into(),
    });

    assert_eq!(
        event,
        LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Phase,
            title: "正在校验 SHA-256".into(),
            detail: "checking SHA-256".into(),
            progress: None,
            message: None,
        }
    );

    let done = launcher_update_event_from_msg(LauncherUpdateMsg::Done);
    assert_eq!(done.kind, LauncherUpdateEventKind::Done);
    assert_eq!(done.title, "自更新完成");
    assert_eq!(done.progress, Some(1.0));
}

#[test]
fn launcher_update_defer_action_preserves_existing_config_semantics() {
    let mut cfg = test_config();

    apply_launcher_update_action(&mut cfg, LauncherUpdateAction::SkipThisVersion, "0.2.0");

    assert_eq!(cfg.skipped_launcher_version.as_deref(), Some("0.2.0"));
    assert_eq!(cfg.known_latest_launcher.as_deref(), Some("0.2.0"));
    assert_eq!(cfg.update_policy, UpdatePolicy::Daily);
}

fn test_config() -> Config {
    Config {
        install_mode: InstallMode::User,
        current_version: "1.0.0".into(),
        update_policy: UpdatePolicy::Daily,
        last_check_unix: None,
        suppress_until_unix: None,
        known_latest: None,
        skipped_version: None,
        keep_versions: 2,
        fetcher: Fetcher::Direct,
        use_current_junction: true,
        register_uninstall: true,
        known_latest_launcher: None,
        skipped_launcher_version: None,
        launcher_suppress_until_unix: None,
    }
}
