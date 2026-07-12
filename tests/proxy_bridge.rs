use codex_windows_cn::bridge::{
    launch_result_from_outcome, persist_version_settings, proxy_launch_status, version_inventory,
    VersionSettingsRequest,
};
use codex_windows_cn::config::{Config, InstallMode, UpdatePolicy};
use codex_windows_cn::proxy::LaunchOutcome;
use codex_windows_cn::store::Fetcher;
use codex_windows_cn::versions::AppKind;
use std::path::{Path, PathBuf};

#[test]
fn proxy_launch_status_reports_latest_installed_codex() {
    let root = TestRoot::new("proxy-launch-success");
    root.write_codex_exe("1.0.0");
    root.write_codex_exe("1.2.0");

    let status = proxy_launch_status(root.path(), &test_config(false));

    assert!(status.managed_install);
    assert_eq!(status.current_version.as_deref(), Some("1.2.0"));
    assert_eq!(status.known_latest.as_deref(), Some("1.2.0"));
    assert!(status.can_launch);
    assert_eq!(status.product_name, "Codex");
    assert!(status.running_versions.is_empty());
    assert!(status
        .codex_exe
        .expect("codex exe path")
        .ends_with(r"1.2.0\Codex.exe"));
    assert_eq!(status.message, "可以启动 Codex");
}

#[test]
fn proxy_launch_status_reports_missing_codex_in_chinese() {
    let root = TestRoot::new("proxy-launch-missing");

    let status = proxy_launch_status(root.path(), &test_config(false));

    assert!(status.managed_install);
    assert_eq!(status.current_version.as_deref(), Some("1.0.0"));
    assert_eq!(status.known_latest.as_deref(), Some("1.2.0"));
    assert!(!status.can_launch);
    assert!(status.codex_exe.is_none());
    assert_eq!(status.product_name, "Codex");
    assert_eq!(status.message, "未找到可启动的 Codex 或 ChatGPT");
}

#[test]
fn proxy_launch_status_prefers_chatgpt_for_unified_packages() {
    let root = TestRoot::new("proxy-launch-chatgpt");
    root.write_codex_exe("26.707.3748.0");
    root.write_exe("26.707.3748.0", "ChatGPT.exe");

    let status = proxy_launch_status(root.path(), &test_config(false));

    assert!(status.can_launch);
    assert_eq!(status.product_name, "ChatGPT");
    assert!(status
        .codex_exe
        .expect("launch path")
        .ends_with(r"26.707.3748.0\ChatGPT.exe"));
    assert_eq!(status.message, "可以启动 ChatGPT");
}

#[test]
fn version_inventory_exposes_default_and_deletion_rules() {
    let root = TestRoot::new("version-inventory");
    root.write_codex_exe("1.0.0");
    root.write_exe("2.0.0", "ChatGPT.exe");

    let inventory = version_inventory(root.path(), &test_config(false)).expect("inventory");

    assert_eq!(inventory.product_name, "ChatGPT");
    assert_eq!(inventory.default_version.as_deref(), Some("2.0.0"));
    assert_eq!(inventory.keep_versions, 2);
    assert!(!inventory.keep_all_versions);
    assert_eq!(inventory.update_policy, UpdatePolicy::Daily);
    assert_eq!(inventory.versions.len(), 2);
    assert!(inventory.versions[0].is_default);
    assert!(inventory.versions.iter().all(|item| item.can_delete));
}

#[test]
fn launch_outcome_becomes_a_structured_switch_request() {
    let result = launch_result_from_outcome(LaunchOutcome::SwitchRequired {
        running_versions: vec!["1.0.0".into()],
        target_version: "2.0.0".into(),
    });

    assert!(!result.launched);
    assert!(result.switch_required);
    assert_eq!(result.version.as_deref(), Some("2.0.0"));
    assert_eq!(result.running_versions, vec!["1.0.0"]);
}

#[test]
fn launched_outcome_reports_the_resolved_product() {
    let result = launch_result_from_outcome(LaunchOutcome::Launched {
        version: "26.707.3748.0".into(),
        app_kind: AppKind::ChatGpt,
    });

    assert!(result.launched);
    assert!(!result.switch_required);
    assert_eq!(result.product_name.as_deref(), Some("ChatGPT"));
    assert_eq!(result.message, "已启动 ChatGPT 26.707.3748.0");
}

#[test]
fn version_settings_are_persisted_and_returned_in_inventory() {
    let root = TestRoot::new("retention-persistence");
    root.write_codex_exe("1.0.0");
    let mut cfg = test_config(false);
    cfg.save_runtime(root.path()).expect("save initial config");

    let result = persist_version_settings(
        root.path(),
        &mut cfg,
        VersionSettingsRequest {
            keep_versions: 7,
            keep_all_versions: true,
            update_policy: UpdatePolicy::Always,
        },
    )
    .expect("persist retention settings");

    assert!(result.applied);
    assert_eq!(result.inventory.keep_versions, 7);
    assert!(result.inventory.keep_all_versions);
    assert_eq!(result.inventory.update_policy, UpdatePolicy::Always);
    let saved = Config::load_runtime(root.path()).expect("reload persisted config");
    assert_eq!(saved.keep_versions, 7);
    assert!(saved.keep_all_versions);
    assert_eq!(saved.update_policy, UpdatePolicy::Always);
}

#[test]
fn reenabling_update_checks_clears_never_suppression() {
    let root = TestRoot::new("update-policy-reenable");
    root.write_codex_exe("1.0.0");
    let mut cfg = test_config(false);
    cfg.update_policy = UpdatePolicy::Never;
    cfg.suppress_until_unix = Some(u64::MAX);
    cfg.launcher_suppress_until_unix = Some(u64::MAX);

    persist_version_settings(
        root.path(),
        &mut cfg,
        VersionSettingsRequest {
            keep_versions: 2,
            keep_all_versions: false,
            update_policy: UpdatePolicy::Daily,
        },
    )
    .expect("reenable update checks");

    assert_eq!(cfg.update_policy, UpdatePolicy::Daily);
    assert_eq!(cfg.suppress_until_unix, None);
    assert_eq!(cfg.launcher_suppress_until_unix, None);
}

fn test_config(use_current_junction: bool) -> Config {
    Config {
        install_mode: InstallMode::User,
        current_version: "1.0.0".into(),
        update_policy: UpdatePolicy::default(),
        last_check_unix: None,
        last_launcher_check_unix: None,
        suppress_until_unix: None,
        known_latest: Some("1.2.0".into()),
        skipped_version: None,
        keep_versions: 2,
        keep_all_versions: false,
        fetcher: Fetcher::Direct,
        use_current_junction,
        register_uninstall: true,
        register_codex_protocol: Some(true),
        known_latest_launcher: None,
        skipped_launcher_version: None,
        launcher_suppress_until_unix: None,
    }
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("codex-windows-cn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_codex_exe(&self, version: &str) {
        self.write_exe(version, "Codex.exe");
    }

    fn write_exe(&self, version: &str, name: &str) {
        let version_dir = self.path.join("versions").join(version);
        std::fs::create_dir_all(&version_dir).expect("create version dir");
        std::fs::write(version_dir.join(name), b"test exe").expect("write executable");
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
