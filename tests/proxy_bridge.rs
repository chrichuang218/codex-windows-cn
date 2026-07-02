use codex_windows_cn::bridge::proxy_launch_status;
use codex_windows_cn::config::{Config, InstallMode, UpdatePolicy};
use codex_windows_cn::store::Fetcher;
use std::path::{Path, PathBuf};

#[test]
fn proxy_launch_status_reports_latest_installed_codex() {
    let root = TestRoot::new("proxy-launch-success");
    root.write_codex_exe("1.0.0");
    root.write_codex_exe("1.2.0");

    let status = proxy_launch_status(root.path(), &test_config(false));

    assert!(status.can_launch);
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

    assert!(!status.can_launch);
    assert!(status.codex_exe.is_none());
    assert_eq!(status.message, "未找到可启动的 Codex.exe");
}

fn test_config(use_current_junction: bool) -> Config {
    Config {
        install_mode: InstallMode::User,
        current_version: "1.0.0".into(),
        update_policy: UpdatePolicy::default(),
        last_check_unix: None,
        suppress_until_unix: None,
        known_latest: None,
        skipped_version: None,
        keep_versions: 2,
        fetcher: Fetcher::Direct,
        use_current_junction,
        register_uninstall: true,
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
        let version_dir = self.path.join("versions").join(version);
        std::fs::create_dir_all(&version_dir).expect("create version dir");
        std::fs::write(version_dir.join("Codex.exe"), b"test exe").expect("write Codex.exe");
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
