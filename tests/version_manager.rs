use codex_windows_cn::config::Config;
use codex_windows_cn::versions::{
    delete_and_repair, delete_installed, resolve_launch_target, scan_installed, versions_to_prune,
    AppKind, RetentionPolicy,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[test]
fn scanner_prefers_chatgpt_and_ignores_non_version_directories() {
    let root = TestRoot::new("version-scan");
    root.write_exe("26.623.13972.0", "Codex.exe");
    root.write_exe("26.707.3748.0", "Codex.exe");
    root.write_exe("26.707.3748.0", "ChatGPT.exe");
    root.write_exe("26.623.13972.0 - 副本", "Codex.exe");
    root.write_exe("26.800.0.0.partial", "ChatGPT.exe");
    std::fs::create_dir_all(root.path().join("versions").join("empty"))
        .expect("create empty directory");

    let versions = scan_installed(root.path()).expect("scan versions");

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version, "26.707.3748.0");
    assert_eq!(versions[0].app_kind, AppKind::ChatGpt);
    assert!(versions[0].executable.ends_with("ChatGPT.exe"));
    assert_eq!(versions[1].version, "26.623.13972.0");
    assert_eq!(versions[1].app_kind, AppKind::Codex);
    assert!(versions[1].executable.ends_with("Codex.exe"));
}

#[test]
fn retention_keeps_latest_count_and_never_prunes_a_running_version() {
    let root = TestRoot::new("version-retention");
    for version in ["1.0.0", "2.0.0", "3.0.0", "4.0.0"] {
        root.write_exe(version, "Codex.exe");
    }
    let versions = scan_installed(root.path()).expect("scan versions");
    let running = HashSet::from(["1.0.0".to_string()]);

    let pruned = versions_to_prune(&versions, RetentionPolicy::KeepLatest(2), &running);

    assert_eq!(pruned, vec!["2.0.0"]);
}

#[test]
fn keep_all_never_schedules_automatic_deletion() {
    let root = TestRoot::new("version-keep-all");
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        root.write_exe(version, "Codex.exe");
    }
    let versions = scan_installed(root.path()).expect("scan versions");

    let pruned = versions_to_prune(&versions, RetentionPolicy::KeepAll, &HashSet::new());

    assert!(pruned.is_empty());
}

#[test]
fn explicit_launch_resolves_the_requested_historical_version() {
    let root = TestRoot::new("version-explicit-launch");
    root.write_exe("1.0.0", "Codex.exe");
    root.write_exe("2.0.0", "ChatGPT.exe");

    let target = resolve_launch_target(root.path(), false, Some("1.0.0"))
        .expect("resolve historical target");

    assert_eq!(target.version, "1.0.0");
    assert_eq!(target.app_kind, AppKind::Codex);
    assert!(target.executable.ends_with(r"1.0.0\Codex.exe"));
}

#[test]
fn delete_guards_running_and_final_versions() {
    let root = TestRoot::new("version-delete-guards");
    root.write_exe("1.0.0", "Codex.exe");
    root.write_exe("2.0.0", "ChatGPT.exe");

    let running = HashSet::from(["1.0.0".to_string()]);
    let running_error = delete_installed(root.path(), "1.0.0", &running)
        .expect_err("running version must be protected");
    assert!(running_error.to_string().contains("currently running"));

    delete_installed(root.path(), "1.0.0", &HashSet::new()).expect("delete old version");
    let final_error = delete_installed(root.path(), "2.0.0", &HashSet::new())
        .expect_err("final version must be protected");
    assert!(final_error.to_string().contains("final launchable version"));
}

#[test]
fn legacy_config_defaults_to_five_versions_without_keep_all() {
    let cfg: Config = serde_json::from_value(serde_json::json!({
        "install_mode": "user",
        "current_version": "1.0.0"
    }))
    .expect("deserialize legacy config");

    assert_eq!(cfg.keep_versions, 5);
    assert!(!cfg.keep_all_versions);
}

#[cfg(windows)]
#[test]
fn deleting_the_default_repairs_config_and_current_junction() {
    let root = TestRoot::new("version-delete-repair");
    root.write_exe("1.0.0", "Codex.exe");
    root.write_exe("2.0.0", "ChatGPT.exe");
    let mut cfg: Config = serde_json::from_value(serde_json::json!({
        "install_mode": "portable",
        "current_version": "2.0.0",
        "use_current_junction": true
    }))
    .expect("build config");
    cfg.save_runtime(root.path()).expect("save initial config");
    codex_windows_cn::junction::set_current(root.path(), "2.0.0").expect("create initial junction");

    let repair = delete_and_repair(root.path(), &mut cfg, "2.0.0", &HashSet::new())
        .expect("delete and repair");

    assert_eq!(repair.default_version, "1.0.0");
    assert!(repair.current_repaired);
    assert_eq!(cfg.current_version, "1.0.0");
    let saved = Config::load_runtime(root.path()).expect("reload config");
    assert_eq!(saved.current_version, "1.0.0");
    assert_eq!(
        std::fs::canonicalize(root.path().join("versions").join("current"))
            .expect("canonicalize current"),
        std::fs::canonicalize(root.path().join("versions").join("1.0.0"))
            .expect("canonicalize remaining version")
    );
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("codex-windows-cn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("versions")).expect("create test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_exe(&self, version: &str, name: &str) {
        let version_dir = self.path.join("versions").join(version);
        std::fs::create_dir_all(&version_dir).expect("create version dir");
        std::fs::write(version_dir.join(name), b"test exe").expect("write exe");
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = codex_windows_cn::junction::remove(&self.path.join("versions").join("current"));
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
