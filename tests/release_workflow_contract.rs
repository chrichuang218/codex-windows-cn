#[test]
fn ci_workflow_covers_frontend_rust_and_tauri_builds() {
    let ci = std::fs::read_to_string(".github/workflows/ci.yml").expect("ci workflow should exist");

    for required in [
        "npm --prefix frontend ci",
        "npm --prefix frontend test",
        "npm --prefix frontend run typecheck",
        "npm --prefix frontend run build",
        "cargo fmt --all -- --check",
        "cargo clippy --all-targets -- -D warnings",
        "cargo test --all-targets",
        "cargo tauri build --no-bundle",
    ] {
        assert!(ci.contains(required), "CI should contain: {required}");
    }
}

#[test]
fn release_workflow_publishes_expected_artifacts_and_trust_text() {
    let release = std::fs::read_to_string(".github/workflows/release.yml")
        .expect("release workflow should exist");

    for required in [
        "npm --prefix frontend ci",
        ".\\scripts\\package-release.ps1",
        "dist/codex-launcher.exe",
        "dist/codex-launcher.exe.sha256",
        "body_path: release-notes.md",
        "## 更新内容",
        "gh attestation verify codex-launcher.exe --owner chrichuang218",
        "不是 OpenAI 官方项目",
        "SHA256",
        "GitHub Actions",
        "chrichuang218/codex-windows-cn",
    ] {
        assert!(
            release.contains(required),
            "release workflow should contain: {required}"
        );
    }

    assert!(!release.contains("--owner vaportail"));
    assert!(!release.contains("generate_release_notes"));
    assert!(!release.contains("cargo tauri build --no-bundle"));
    assert!(!release.contains("cargo build --release"));
}

#[test]
fn v1_smoke_record_covers_the_five_main_paths() {
    let smoke =
        std::fs::read_to_string("docs/release/v1-smoke.md").expect("v1 smoke record should exist");

    for required in [
        "Codex Windows 中文助手 v1",
        "安装",
        "代理启动",
        "检查更新/更新",
        "卸载",
        "自更新",
        "cargo test --all-targets",
        "npm --prefix frontend test",
        "cargo tauri build --no-bundle",
    ] {
        assert!(
            smoke.contains(required),
            "smoke record should contain: {required}"
        );
    }
}
