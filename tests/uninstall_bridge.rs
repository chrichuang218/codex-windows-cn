use codex_windows_cn::bridge::{
    uninstall_confirmation, uninstall_event_from_msg, uninstall_status_for_root, UninstallEvent,
    UninstallEventKind, UninstallStatusKind,
};
use codex_windows_cn::uninstall::UninstallMsg;
use std::path::{Path, PathBuf};

#[test]
fn uninstall_confirmation_explains_deleted_and_preserved_data() {
    let confirmation = uninstall_confirmation(Path::new(r"C:\Users\tester\AppData\Local\Codex"));

    assert_eq!(confirmation.title, "确认卸载 Codex Windows 中文助手");
    assert_eq!(confirmation.root, r"C:\Users\tester\AppData\Local\Codex");
    assert!(confirmation
        .delete_items
        .contains(&"已安装的 Codex 版本".into()));
    assert!(confirmation.delete_items.contains(&"下载缓存".into()));
    assert!(confirmation.delete_items.contains(&"启动器配置".into()));
    assert!(confirmation
        .preserve_items
        .contains(&"Codex 登录数据".into()));
    assert!(confirmation
        .preserve_items
        .contains(&"日志和诊断信息".into()));
}

#[test]
fn uninstall_status_rejects_a_directory_without_the_install_signature() {
    let root = TestRoot::new("uninstall-missing-signature");

    let status = uninstall_status_for_root(root.path());

    assert_eq!(status.kind, UninstallStatusKind::Blocked);
    assert_eq!(status.title, "无法卸载");
    assert!(status.message.contains("拒绝卸载"));
}

#[test]
fn uninstall_worker_messages_are_reported_as_chinese_events() {
    let event = uninstall_event_from_msg(UninstallMsg::Done {
        log_path: r"C:\Temp\codex-uninstall.log".into(),
    });

    assert_eq!(
        event,
        UninstallEvent {
            kind: UninstallEventKind::Done,
            title: "卸载完成".into(),
            detail: "卸载日志：C:\\Temp\\codex-uninstall.log".into(),
            progress: Some(1.0),
            log_path: Some(r"C:\Temp\codex-uninstall.log".into()),
            message: None,
        }
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
        std::fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
