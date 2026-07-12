//! `--uninstall` handler. Defensive flow with guardrails + user consent.
//!
//! Split in two layers:
//!
//! - `load_context` / `need_elevation`  — parse state from disk, decide if
//!   the caller needs to self-elevate. Cheap, synchronous, no side effects.
//! - `run_worker`                        — the actual destructive flow, with
//!   progress callbacks so the caller can drive a UI.
//!
//! main.rs composes these:
//!   1. load_context (if it fails — not our install — bail early)
//!   2. need_elevation check → respawn elevated and exit
//!   3. Open a Slint window on screen 20 (confirm)
//!   4. On user confirm → screen 21 + spawn `run_worker` on a thread
//!   5. Worker messages drive screens 21/22/23
//!
//! Worker flow:
//!   a. `safety::validate_uninstall_root` → error on refusal
//!   b. MessageBox prompt if any Codex.exe processes are alive → abort or kill
//!   c. Remove shortcut / registry / junction
//!   d. Whitelist delete of versions/, downloads/, updater.json
//!   e. POSIX self-delete of the launcher (reboot-delete fallback)
//!   f. Best-effort `rmdir` of the install root
//!   g. Write %TEMP%\codex-uninstall-<ts>.log
//!
//! See POSTMORTEM_phase7_uninstaller.md for the incident that motivated the
//! current design.

use crate::cleanup::{self, CleanupReport};
use crate::config::{Config, InstallMode, CONFIG_FILENAME};
use crate::{dialogs, elevate, installer, junction, proxy, registry, safety};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Progress messages the worker posts to the UI layer.
#[derive(Debug, Clone)]
pub enum UninstallMsg {
    Phase { phase: String, detail: String },
    Progress(Option<f32>),
    Done { log_path: String },
    Error(String),
}

pub struct UninstallContext {
    pub root: PathBuf,
    pub cfg: Config,
}

/// Load the install root + config from next to the exe. Returns `Err` if
/// no `updater.json` exists (or it doesn't parse) — signals this isn't
/// our install and caller should abort.
pub fn load_context() -> Result<UninstallContext> {
    let exe = std::env::current_exe().context("current_exe")?;
    let root = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exe has no parent directory"))?
        .to_path_buf();
    let cfg = Config::load(&root.join(CONFIG_FILENAME))
        .context("loading updater.json next to launcher")?;
    Ok(UninstallContext { root, cfg })
}

/// True if uninstall requires admin and we don't have it — caller should
/// respawn elevated. HKLM registry delete + Program Files removal need admin.
pub fn need_elevation(ctx: &UninstallContext) -> bool {
    matches!(ctx.cfg.install_mode, InstallMode::System) && !elevate::is_elevated()
}

/// Legacy headless entry — kept so `--uninstall` without a wired UI still
/// works (e.g. scripted invocation). Does everything through MessageBox.
///
/// Main.rs prefers the UI-driven path; this is the fallback.
#[allow(dead_code)]
pub fn run() -> Result<()> {
    let ctx = load_context()?;
    if matches!(ctx.cfg.install_mode, InstallMode::System) && elevate::is_elevated() {
        anyhow::bail!("系统卸载必须从普通用户进程启动，才能清理发起用户的 codex:// 关联");
    }
    if need_elevation(&ctx) {
        let root = ctx.root.clone();
        let exit_code = elevate::respawn_elevated_wait("--uninstall-elevated")?;
        if exit_code != 0 {
            anyhow::bail!("elevated uninstall failed with exit code {exit_code}");
        }
        registry::remove_codex_protocol_if_owned(InstallMode::User, &root)
            .context("removing current-user codex:// registration after system uninstall")?;
        return Ok(());
    }

    run_worker_to_completion(ctx, |msg| eprintln!("{msg:?}"))
        .map(|_| ())
        .map_err(anyhow::Error::msg)
}

pub fn run_elevated() -> Result<()> {
    if !elevate::is_elevated() {
        anyhow::bail!("系统卸载 helper 需要管理员权限");
    }
    let ctx = load_context()?;
    if !matches!(ctx.cfg.install_mode, InstallMode::System) {
        anyhow::bail!("当前安装不是所有用户安装");
    }
    run_worker_to_completion(ctx, |msg| eprintln!("{msg:?}"))
        .map(|_| ())
        .map_err(anyhow::Error::msg)
}

pub fn run_worker_to_completion(
    ctx: UninstallContext,
    on_msg: impl Fn(UninstallMsg),
) -> std::result::Result<String, String> {
    let outcome = std::sync::Mutex::new(None);
    run_worker(ctx, |msg| {
        let terminal = match &msg {
            UninstallMsg::Done { log_path } => Some(Ok(log_path.clone())),
            UninstallMsg::Error(message) => Some(Err(message.clone())),
            _ => None,
        };
        on_msg(msg);
        if let Some(terminal) = terminal {
            if let Ok(mut current) = outcome.lock() {
                *current = Some(terminal);
            }
        }
    });
    outcome
        .into_inner()
        .map_err(|_| "读取卸载结果失败".to_string())?
        .ok_or_else(|| "卸载进程未返回最终结果".to_string())?
}

/// Destructive worker. `on_msg` is the progress callback — posts `UninstallMsg`
/// values the UI uses to advance screens. Caller is responsible for ensuring
/// `on_msg` is safe to call from a background thread.
pub fn run_worker(ctx: UninstallContext, on_msg: impl Fn(UninstallMsg)) {
    let root = &ctx.root;

    // --- 1. guardrail ------------------------------------------------------
    on_msg(UninstallMsg::Phase {
        phase: "Validating install".into(),
        detail: root.display().to_string(),
    });
    on_msg(UninstallMsg::Progress(None));
    if let Err(e) = safety::validate_uninstall_root(root) {
        on_msg(UninstallMsg::Error(format!(
            "Refused to uninstall: {e}\n\n\
             No files have been modified."
        )));
        return;
    }

    // --- 2. running-Codex prompt ------------------------------------------
    // Only consider Codex processes from *this* install. Foreign Codex
    // installs and unrelated `codex.exe` binaries are left alone.
    let versions_root = root.join("versions");
    let pids = proxy::find_our_codex_pids(&versions_root);
    if !pids.is_empty() {
        let msg = format!(
            "Codex is currently running ({} process{}).\n\n\
             Terminate it and continue uninstalling?\n\n\
             Click No to cancel. No files have been modified yet.",
            pids.len(),
            if pids.len() == 1 { "" } else { "es" }
        );
        if !dialogs::yes_no("Codex is running", &msg) {
            on_msg(UninstallMsg::Error(
                "Uninstall cancelled — Codex is still running.".into(),
            ));
            return;
        }
        on_msg(UninstallMsg::Phase {
            phase: "Terminating Codex".into(),
            detail: format!("{} process(es)", pids.len()),
        });
        proxy::terminate_pids(&pids, 5000);
        let still = proxy::find_our_codex_pids(&versions_root);
        if !still.is_empty() {
            on_msg(UninstallMsg::Error(format!(
                "Failed to terminate {} Codex process(es). Aborting — no files modified.",
                still.len()
            )));
            return;
        }
    }

    // --- 3. destructive actions -------------------------------------------
    on_msg(UninstallMsg::Phase {
        phase: "Removing shortcuts".into(),
        detail: "".into(),
    });
    if let Err(cause) = installer::remove_start_menu_shortcut(&ctx.root, ctx.cfg.install_mode) {
        on_msg(UninstallMsg::Error(format!(
            "移除开始菜单快捷方式失败：{cause:#}"
        )));
        return;
    }
    if let Err(cause) = installer::remove_desktop_shortcut(&ctx.root, ctx.cfg.install_mode) {
        on_msg(UninstallMsg::Error(format!(
            "移除 ChatGPT 桌面快捷方式失败：{cause:#}"
        )));
        return;
    }
    if let Err(cause) =
        installer::remove_assistant_desktop_shortcut(&ctx.root, ctx.cfg.install_mode)
    {
        on_msg(UninstallMsg::Error(format!(
            "移除中文助手桌面快捷方式失败：{cause:#}"
        )));
        return;
    }

    on_msg(UninstallMsg::Phase {
        phase: "Removing registry entries".into(),
        detail: "".into(),
    });
    if let Err(cause) = registry::remove_codex_protocol_if_owned(ctx.cfg.install_mode, root) {
        on_msg(UninstallMsg::Error(format!(
            "移除 codex:// 会话链接失败：{cause:#}\n\n已停止卸载，程序文件尚未删除。"
        )));
        return;
    }
    let _ = registry::remove(ctx.cfg.install_mode);

    on_msg(UninstallMsg::Phase {
        phase: "Removing versions/current junction".into(),
        detail: "".into(),
    });
    let _ = junction::remove(&root.join("versions").join("current"));

    on_msg(UninstallMsg::Phase {
        phase: "Deleting files".into(),
        detail: "".into(),
    });
    let mut report = CleanupReport::new();
    whitelist_delete(root, &mut report, &on_msg);

    on_msg(UninstallMsg::Phase {
        phase: "Finalizing".into(),
        detail: "".into(),
    });
    report.self_delete = cleanup::delete_self_exe();

    match cleanup::retry_delete_dir_only(root) {
        Ok(()) => report.deleted.push(root.clone()),
        Err(e) => report
            .skipped
            .push((root.clone(), format!("root rmdir: {e}"))),
    }

    let log_path = write_report(root, &report)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    on_msg(UninstallMsg::Done { log_path });
}

/// Delete only the files/dirs we know we placed. Anything else in `root`
/// (Codex logs, user data, foreign files) is left alone.
fn whitelist_delete(root: &Path, report: &mut CleanupReport, on_msg: &impl Fn(UninstallMsg)) {
    on_msg(UninstallMsg::Phase {
        phase: "Deleting versioned installs".into(),
        detail: "".into(),
    });
    cleanup::retry_delete_dir_all(&root.join("versions"), report);

    on_msg(UninstallMsg::Phase {
        phase: "Deleting download cache".into(),
        detail: "".into(),
    });
    cleanup::retry_delete_dir_all(&root.join("downloads"), report);

    on_msg(UninstallMsg::Phase {
        phase: "Removing config".into(),
        detail: "".into(),
    });
    let cfg = root.join(CONFIG_FILENAME);
    match cleanup::retry_delete_file(&cfg) {
        Ok(()) => report.deleted.push(cfg),
        Err(e) => report.skipped.push((cfg, format!("{e}"))),
    }

    // Per-user runtime-state fallback — only ours. Match on embedded
    // install_root so we don't wipe another install's state. launcher.log
    // in the same dir is left for post-uninstall diagnostics.
    match crate::config::clear_state_file_if_ours(root) {
        Ok(Some(p)) => report.deleted.push(p),
        Ok(None) => {}
        Err(e) => report
            .skipped
            .push((std::path::PathBuf::from("state.json"), format!("{e}"))),
    }
}

fn write_report(root: &Path, report: &CleanupReport) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let log_path = std::env::temp_dir().join(format!("codex-uninstall-{ts}.log"));
    std::fs::write(&log_path, report.to_log_string(root))
        .with_context(|| format!("writing {}", log_path.display()))?;
    Ok(log_path)
}
