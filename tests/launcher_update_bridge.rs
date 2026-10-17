use codex_windows_cn::bridge::{
    apply_launcher_update_action, launcher_update_event_from_msg,
    launcher_update_status_from_decision, LauncherUpdateAction, LauncherUpdateEvent,
    LauncherUpdateEventKind, LauncherUpdateStatusKind,
};
use codex_windows_cn::config::{Config, InstallMode, UpdatePolicy};
use codex_windows_cn::launcher_update::LauncherUpdateMsg;
use codex_windows_cn::store::Fetcher;
use codex_windows_cn::updater::{
    LauncherDecision, LAUNCHER_LATEST_API, LAUNCHER_LATEST_RELEASE_URL, LAUNCHER_REPO,
};

#[test]
fn launcher_release_source_uses_this_project_repository() {
    assert_eq!(LAUNCHER_REPO, "chrichuang218/codex-windows-cn");
    assert_eq!(
        LAUNCHER_LATEST_RELEASE_URL,
        "https://github.com/chrichuang218/codex-windows-cn/releases/latest"
    );
    assert_eq!(
        LAUNCHER_LATEST_API,
        "https://api.github.com/repos/chrichuang218/codex-windows-cn/releases/latest"
    );
}

#[test]
fn normal_startup_cleans_previous_launcher_artifacts() {
    let source = include_str!("../src/main.rs");
    let helper_exit = source
        .find("if let Some(exit_code) = run_cli_helper()")
        .expect("main should preserve CLI helper handling");
    let cleanup = source
        .find("launcher_update::cleanup_stale_launchers(dir);")
        .expect("normal startup should clean stale launcher artifacts");
    let single_instance = source
        .find("let Some(_single_instance_guard) = claim_single_instance_or_focus_existing()")
        .expect("main should preserve single-instance handling");

    assert!(single_instance > helper_exit);
    assert!(cleanup > single_instance);
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
fn launcher_rate_limit_error_becomes_short_chinese_status() {
    let status = launcher_update_status_from_decision(LauncherDecision::Error(
        "HTTP status client error (403 rate limit exceeded) for url (https://api.github.com/repos/chrichuang218/codex-windows-cn/releases/latest)".into(),
    ));

    assert_eq!(status.kind, LauncherUpdateStatusKind::Error);
    assert_eq!(status.title, "检查启动器更新受限");
    assert_eq!(
        status.message,
        "GitHub 暂时限制了未登录接口请求，稍后会自动重试。"
    );
    assert_eq!(status.actions, Vec::<LauncherUpdateAction>::new());
}

#[test]
fn launcher_skipped_reason_becomes_short_chinese_status() {
    let status = launcher_update_status_from_decision(LauncherDecision::Skipped {
        reason: "update_policy = never".into(),
    });

    assert_eq!(status.kind, LauncherUpdateStatusKind::Skipped);
    assert_eq!(status.message, "已关闭自动检查更新");

    let unknown = launcher_update_status_from_decision(LauncherDecision::Skipped {
        reason: "unexpected internal reason".into(),
    });
    assert_eq!(unknown.message, "暂不检查启动器更新");
}

#[test]
fn launcher_update_worker_messages_are_reported_as_chinese_events() {
    let event = launcher_update_event_from_msg(LauncherUpdateMsg::Phase {
        phase: "Verifying".into(),
        detail: "核对发布页提供的 SHA-256 校验值".into(),
    });

    assert_eq!(
        event,
        LauncherUpdateEvent {
            kind: LauncherUpdateEventKind::Phase,
            title: "正在校验 SHA-256".into(),
            detail: "核对发布页提供的 SHA-256 校验值".into(),
            progress: None,
            message: None,
        }
    );

    let done = launcher_update_event_from_msg(LauncherUpdateMsg::Done {
        version: "0.2.0".into(),
    });
    assert_eq!(done.kind, LauncherUpdateEventKind::Done);
    assert_eq!(done.title, "自更新完成");
    assert_eq!(done.detail, "已更新到 v0.2.0，重启后生效");
    assert_eq!(done.progress, Some(1.0));
}

#[test]
fn launcher_update_errors_show_chinese_summary_and_keep_raw_detail() {
    let cases = [
        (
            "SHA-256 mismatch: expected aa, got bb. Aborting; running launcher untouched.",
            "下载的文件校验失败，当前版本未改动。",
        ),
        (
            "smoke-test failed: new launcher exited with exit code: 1",
            "新版本自检未通过，当前版本未改动。",
        ),
        (
            "replacing running launcher: moving a → b: Access is denied.",
            "替换启动器文件失败，可能没有写入权限，当前版本未改动。",
        ),
        (
            "downloading https://github.com/x/codex-launcher.exe: error sending request",
            "下载新版本失败，请检查网络后重试。当前版本未改动。",
        ),
        (
            "fetching sha: HTTP status client error (403 Forbidden): API rate limit exceeded",
            "GitHub 暂时限制了未登录接口请求，请稍后重试。当前版本未改动。",
        ),
        ("something unexpected", "自更新失败，当前版本未改动。"),
    ];

    for (raw, summary) in cases {
        let event = launcher_update_event_from_msg(LauncherUpdateMsg::Error(raw.into()));
        assert_eq!(event.kind, LauncherUpdateEventKind::Error);
        assert_eq!(event.message.as_deref(), Some(summary), "raw: {raw}");
        assert_eq!(event.detail, raw);
    }
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
