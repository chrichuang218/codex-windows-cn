#[test]
fn readme_states_the_v1_chinese_product_and_trust_boundary() {
    let readme = std::fs::read_to_string("README.md").expect("README.md should exist");

    for required in [
        "Codex Windows 中文助手",
        "中文安装更新助手",
        "五条主路径",
        "安装",
        "代理启动",
        "检查更新/更新",
        "卸载",
        "自更新",
        "不是 OpenAI 官方项目",
        "不修改、不重新分发 Codex 本体",
        "官方 Microsoft Store MSIX",
        "SHA256",
        "GitHub Actions",
        "gh attestation verify codex-launcher.exe --owner chrichuang218",
        "chrichuang218/codex-windows-cn",
        "LINUX DO",
        "vaportail/codex-windows-updater",
    ] {
        assert!(
            readme.contains(required),
            "README should contain required statement: {required}"
        );
    }
}

#[test]
fn readme_does_not_keep_old_slint_attribution() {
    let readme = std::fs::read_to_string("README.md").expect("README.md should exist");

    for forbidden in [
        "Made with Slint",
        "Slint Royalty-Free License",
        "slint-ui/slint",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README should not contain stale statement: {forbidden}"
        );
    }
}

#[test]
fn package_metadata_stays_within_v1_product_boundary() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should exist");

    assert!(manifest.contains("Codex Windows Chinese installer"));
    assert!(
        !manifest.contains("repair assistant"),
        "package metadata should not advertise deferred repair features"
    );
}

#[test]
fn windows_integration_metadata_uses_this_project_identity() {
    let installer = std::fs::read_to_string("src/installer.rs").expect("installer source exists");

    assert!(installer.contains("Codex Windows 中文助手"));
    assert!(installer.contains("publisher: \"chrichuang218\""));
    assert!(!installer.contains("Codex (unofficial updater)"));
    assert!(!installer.contains("publisher: \"vaportail\""));
}
