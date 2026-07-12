//! Installer orchestration. Runs in a worker thread so the Slint event
//! loop stays responsive; posts [`InstallMsg`] updates back via a
//! user-provided callback.
//!
//! Steps executed:
//!   1. Download MSIX (Direct or Winget) into `<root>/downloads/`
//!   2. Extract `app/` subtree into `<root>/versions/<ver>/`
//!   3. Copy current launcher exe to `<root>/codex-launcher.exe` (so the
//!      stub lives next to updater.json and can proxy-launch later)
//!   4. Write `<root>/updater.json`
//!   5. Prune old versions to `keep_versions`
//!
//! Shortcuts / registry / UAC elevation are intentionally NOT here — they
//! live in their own modules alongside the rest of the Windows-integration goo.

use crate::config::{Config, InstallMode, UpdatePolicy};
use crate::dialogs;
use crate::extract;
use crate::junction;
use crate::registry;
use crate::shortcut;
use crate::store::{self, Fetcher};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const MIN_UPDATE_EXTRACT_VISIBLE: Duration = Duration::from_millis(900);

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub create_shortcut: bool,
    pub create_desktop_shortcut: bool,
    pub create_assistant_desktop_shortcut: bool,
    /// Write the Add/Remove Programs registry entry so the user can
    /// uninstall via Windows Settings → Apps. Off by default for Portable.
    pub register_uninstall: bool,
    /// Maintain the `codex://` URL protocol for CLI `/app` handoff.
    pub register_codex_protocol: bool,
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub fetcher: Fetcher,
    /// Create `versions/current` junction pointing at the newest install.
    pub use_current_junction: bool,
    /// For LocalFile fetcher — user-supplied MSIX path. Ignored otherwise.
    pub local_msix: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum InstallMsg {
    /// Phase name (e.g. "Downloading", "Extracting") + detail line.
    Phase { phase: String, detail: String },
    /// Fraction in [0.0, 1.0]. Use `None` for indeterminate.
    Progress(Option<f32>),
    /// Progress with its own phase/detail so polling the last event is enough
    /// to render a useful UI.
    ProgressDetail {
        phase: String,
        detail: String,
        progress: Option<f32>,
    },
    /// Installation finished successfully. Carries the installed version.
    Done { version: String },
    /// Installation failed. Carries a user-readable error message.
    Error(String),
}

/// Default install path for a given mode. Caller can override via UI.
pub fn default_path(mode: InstallMode) -> PathBuf {
    match mode {
        InstallMode::Portable => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("CodexPortable"),
        InstallMode::User => directories::BaseDirs::new()
            .map(|d| d.data_local_dir().join("Codex"))
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public\Codex")),
        InstallMode::System => PathBuf::from(r"C:\Program Files\Codex"),
    }
}

/// Synchronous install. Call from a worker thread. `on_msg` is invoked
/// with phase/progress updates AND the final Done/Error — callers should
/// treat a Done or Error as the last message they'll ever see.
pub fn run(opts: InstallOptions, on_msg: impl Fn(InstallMsg) + Send + 'static) {
    match run_inner(&opts, &on_msg, None, PostWorkMode::Background) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

pub fn run_cancellable(
    opts: InstallOptions,
    cancel: Arc<AtomicBool>,
    on_msg: impl Fn(InstallMsg) + Send + 'static,
) {
    match run_inner(
        &opts,
        &on_msg,
        Some(cancel.as_ref()),
        PostWorkMode::Background,
    ) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

/// Run a System install inside the one-shot elevated helper. Post-install
/// Windows integration must finish before the helper process exits.
pub fn run_elevated(opts: InstallOptions) -> Result<String> {
    run_inner(&opts, &|_| {}, None, PostWorkMode::Inline)
}

/// Update an existing install in-place. Loads `<root>/updater.json`, downloads
/// and extracts the latest MSIX, updates `current_version`, `known_latest`,
/// and `last_check_unix`, then prunes. Does NOT replace the launcher exe
/// (we're running from it). Does NOT change install_mode / keep_versions / etc.
pub fn update(root: std::path::PathBuf, on_msg: impl Fn(InstallMsg) + Send + 'static) {
    match update_inner(&root, &on_msg, PostWorkMode::Background) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

/// Run a System update inside the one-shot elevated helper. Post-update
/// Windows integration must finish before the helper process exits.
pub fn update_elevated(root: PathBuf) -> Result<String> {
    update_inner(&root, &|_| {}, PostWorkMode::Inline)
}

#[derive(Clone, Copy)]
enum PostWorkMode {
    Background,
    Inline,
}

fn update_inner(
    root: &Path,
    on_msg: &dyn Fn(InstallMsg),
    post_work_mode: PostWorkMode,
) -> Result<String> {
    // load_runtime, not load — the per-user state overlay (System installs)
    // holds runtime-current values like update_policy, skipped_version,
    // launcher_suppress_until_unix. save_install below clears the overlay,
    // so we must merge it in first or those choices vanish post-update.
    let mut cfg = Config::load_runtime(root)
        .with_context(|| format!("loading existing config at {}", root.display()))?;

    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    let update_fetcher = configured_update_fetcher(cfg.fetcher);
    emit_progress_detail(
        on_msg,
        "Downloading",
        &update_start_detail(update_fetcher),
        None,
    );

    let boxed = BoxedFn(on_msg, None);
    // Discard `_succeeded_via`: a single transient failure (offline, proxy
    // blip) shouldn't permanently demote the user's chosen fetcher. A
    // stronger signal (N consecutive failures) would be needed first.
    let (result, _succeeded_via) = store::download_latest_with_fallback(
        update_fetcher,
        store::PRODUCT_ID_CODEX,
        &downloads,
        &mut |done, total| boxed.progress_bytes(done, total),
    )?;

    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    );

    let extract_started = Instant::now();
    let boxed = BoxedFn(on_msg, None);
    extract::extract_app(
        &result.msix_path,
        root,
        &result.version,
        &mut |done, total| boxed.progress_entries(done, total),
    )?;
    let elapsed = extract_started.elapsed();
    if elapsed < MIN_UPDATE_EXTRACT_VISIBLE {
        std::thread::sleep(MIN_UPDATE_EXTRACT_VISIBLE - elapsed);
    }

    emit_progress_detail(on_msg, "Finalizing", "正在写入更新配置。", None);

    // Reload just before committing so settings changed while the large
    // download/extract was running are preserved instead of being overwritten
    // by the snapshot loaded at update start.
    cfg = Config::load_runtime(root)
        .with_context(|| format!("reloading runtime config at {}", root.display()))?;
    cfg.current_version = result.version.clone();
    cfg.known_latest = Some(result.version.clone());
    cfg.suppress_until_unix = None;
    cfg.last_check_unix = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    // Update flow is elevated for System (UAC re-spawn) and runs in user
    // context for User/Portable, so install-root write is always available
    // here. save_install also clears any stale per-user state overlay so
    // the freshly-written install-root config takes effect on next launch.
    cfg.save_install(root)?;

    if let Err(error) = sync_launch_surface(
        root,
        cfg.install_mode,
        cfg.use_current_junction,
        cfg.register_codex_protocol_preference(),
    ) {
        report_protocol_sync_error("更新", &error);
    }

    // At this point the update is usable: the new version is extracted and
    // updater.json points at it. Keep Windows shell integration and cleanup
    // out of the critical path so slow AV / registry / shortcut work cannot
    // leave the UI stuck on the final progress screen.
    let post_update = PostUpdateWork {
        root: root.to_path_buf(),
        version: result.version.clone(),
        mode: cfg.install_mode,
        register_uninstall: cfg.register_uninstall,
        keep_versions: cfg.keep_versions,
        keep_all_versions: cfg.keep_all_versions,
        use_current_junction: cfg.use_current_junction,
        msix_path: result.msix_path.clone(),
    };
    match post_work_mode {
        PostWorkMode::Background => {
            std::thread::spawn(move || run_post_update_work(post_update));
        }
        PostWorkMode::Inline => run_post_update_work(post_update),
    }

    Ok(result.version)
}

fn run_inner(
    opts: &InstallOptions,
    on_msg: &dyn Fn(InstallMsg),
    cancel: Option<&AtomicBool>,
    post_work_mode: PostWorkMode,
) -> Result<String> {
    check_cancel(cancel)?;
    std::fs::create_dir_all(&opts.root)
        .with_context(|| format!("creating install root {}", opts.root.display()))?;
    let downloads = opts.root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    // --- 1. Download --------------------------------------------------------
    emit_progress_detail(
        on_msg,
        "Downloading",
        &download_start_detail(opts.fetcher),
        None,
    );

    let result = match opts.fetcher {
        Fetcher::LocalFile => {
            check_cancel(cancel)?;
            let src = opts
                .local_msix
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("LocalFile fetcher needs a file path"))?;
            let on_msg_clone = BoxedFn(on_msg, cancel);
            store::local_file::from_file(src, &downloads, &mut |done, total| {
                on_msg_clone.progress_bytes(done, total)
            })?
        }
        _ => {
            check_cancel(cancel)?;
            let on_msg_clone = BoxedFn(on_msg, cancel);
            // See the note in `update_inner` for why we discard `_succeeded_via`.
            let (r, _succeeded_via) = store::download_latest_with_fallback(
                opts.fetcher,
                store::PRODUCT_ID_CODEX,
                &downloads,
                &mut |done, total| on_msg_clone.progress_bytes(done, total),
            )?;
            r
        }
    };

    // --- 2. Extract ---------------------------------------------------------
    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    );

    check_cancel(cancel)?;
    let on_msg_clone = BoxedFn(on_msg, cancel);
    extract::extract_app(
        &result.msix_path,
        &opts.root,
        &result.version,
        &mut |done, total| on_msg_clone.progress_entries(done, total),
    )?;

    // --- 3. Place launcher stub --------------------------------------------
    emit_progress_detail(on_msg, "Finalizing", "正在写入启动器和配置。", None);

    check_cancel(cancel)?;
    place_launcher(&opts.root)?;

    // --- 4. Write config ----------------------------------------------------
    let cfg = Config {
        install_mode: opts.mode,
        current_version: result.version.clone(),
        update_policy: UpdatePolicy::default(),
        last_check_unix: None,
        last_launcher_check_unix: None,
        suppress_until_unix: None,
        known_latest: Some(result.version.clone()),
        skipped_version: None,
        keep_versions: opts.keep_versions.max(1),
        keep_all_versions: opts.keep_all_versions,
        fetcher: match opts.fetcher {
            Fetcher::LocalFile => Fetcher::Direct, // don't persist LocalFile
            f => f,
        },
        use_current_junction: opts.use_current_junction,
        register_uninstall: opts.register_uninstall,
        register_codex_protocol: Some(opts.register_codex_protocol),
        known_latest_launcher: None,
        skipped_launcher_version: None,
        launcher_suppress_until_unix: None,
    };
    // Initial install runs elevated for System mode, so install-root
    // write always succeeds. save_install also clears any stale state
    // overlay from a previous install at this same root.
    check_cancel(cancel)?;
    cfg.save_install(&opts.root)?;

    if let Err(error) = sync_launch_surface(
        &opts.root,
        cfg.install_mode,
        cfg.use_current_junction,
        cfg.register_codex_protocol_preference(),
    ) {
        report_protocol_sync_error("安装", &error);
    }

    // --- 5. Best-effort Windows integration + cleanup -----------------------
    //
    // At this point the install is usable: Codex is extracted, the launcher is
    // in place, and updater.json is written. Junctions, shortcuts, registry
    // metadata, pruning, and MSIX cleanup are useful polish, but should not be
    // allowed to keep the UI stuck on the final progress screen if Windows
    // shell integration or antivirus is slow.
    let post_install = PostInstallWork {
        root: opts.root.clone(),
        version: result.version.clone(),
        mode: opts.mode,
        create_shortcut: opts.create_shortcut,
        create_desktop_shortcut: opts.create_desktop_shortcut,
        create_assistant_desktop_shortcut: opts.create_assistant_desktop_shortcut,
        register_uninstall: opts.register_uninstall,
        keep_versions: cfg.keep_versions,
        keep_all_versions: cfg.keep_all_versions,
        use_current_junction: cfg.use_current_junction,
        msix_path: result.msix_path.clone(),
    };
    match post_work_mode {
        PostWorkMode::Background => {
            std::thread::spawn(move || run_post_install_work(post_install));
        }
        PostWorkMode::Inline => run_post_install_work(post_install),
    }

    Ok(result.version)
}

struct PostInstallWork {
    root: PathBuf,
    version: String,
    mode: InstallMode,
    create_shortcut: bool,
    create_desktop_shortcut: bool,
    create_assistant_desktop_shortcut: bool,
    register_uninstall: bool,
    keep_versions: u32,
    keep_all_versions: bool,
    use_current_junction: bool,
    msix_path: PathBuf,
}

fn run_post_install_work(work: PostInstallWork) {
    if work.create_shortcut {
        if let Err(e) = write_start_menu_shortcut(&work.root, work.mode) {
            eprintln!("warn: shortcut: {e:#}");
            dialogs::error(&format!(
                "创建中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
            ));
        }
    }
    if work.create_desktop_shortcut {
        if let Err(e) =
            create_or_update_desktop_shortcut(&work.root, work.mode, work.use_current_junction)
        {
            eprintln!("warn: desktop shortcut: {e:#}");
            dialogs::error(&format!(
                "创建 ChatGPT 桌面快捷方式失败：{e:#}\n\n可稍后在中文助手的设置页重试。"
            ));
        }
    }
    if work.create_assistant_desktop_shortcut {
        if let Err(e) = create_or_update_assistant_desktop_shortcut(&work.root, work.mode) {
            eprintln!("warn: assistant desktop shortcut: {e:#}");
            dialogs::error(&format!(
                "创建中文助手桌面快捷方式失败：{e:#}\n\n可稍后在中文助手的设置页重试。"
            ));
        }
    }
    if work.register_uninstall {
        if let Err(e) = write_registry(&work.root, work.mode, &work.version) {
            eprintln!("warn: registry uninstall entry: {e:#}");
        }
    }
    prune_versions(&work.root, work.keep_versions, work.keep_all_versions);
    let _ = std::fs::remove_file(&work.msix_path);
}

struct PostUpdateWork {
    root: PathBuf,
    version: String,
    mode: InstallMode,
    register_uninstall: bool,
    keep_versions: u32,
    keep_all_versions: bool,
    use_current_junction: bool,
    msix_path: PathBuf,
}

fn sync_launch_surface_with(
    root: &Path,
    use_current_junction: bool,
    mut sync_protocol: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let current = root.join("versions").join("current");
    if !use_current_junction {
        junction::remove(&current)?;
    }
    let target = crate::versions::resolve_launch_target(root, use_current_junction, None)?;
    sync_protocol(&target.executable)
}

fn sync_launch_surface(
    root: &Path,
    mode: InstallMode,
    use_current_junction: bool,
    preference: Option<bool>,
) -> Result<()> {
    sync_launch_surface_with(root, use_current_junction, |handler| {
        match preference {
            Some(true) => {
                if matches!(
                    registry::register_codex_protocol(mode, root, handler)?,
                    registry::ProtocolRegistration::PreservedForeign
                ) {
                    eprintln!("warn: codex:// is owned by another installation; preserving it");
                }
            }
            Some(false) => {
                let _ = registry::remove_codex_protocol_if_owned(mode, root)?;
            }
            None => {
                if matches!(
                    registry::codex_protocol_status(mode, root, handler)?,
                    registry::CodexProtocolStatus::NeedsRepair
                ) {
                    let _ = registry::register_codex_protocol(mode, root, handler)?;
                }
            }
        }
        Ok(())
    })
}

pub fn sync_codex_protocol(root: &Path, cfg: &Config) -> Result<()> {
    sync_launch_surface(
        root,
        cfg.install_mode,
        cfg.use_current_junction,
        cfg.register_codex_protocol_preference(),
    )
}

pub fn codex_protocol_status(root: &Path, cfg: &Config) -> Result<registry::CodexProtocolStatus> {
    let target = crate::versions::resolve_launch_target(root, cfg.use_current_junction, None)?;
    registry::codex_protocol_status(cfg.install_mode, root, &target.executable)
}

pub fn enable_codex_protocol(
    root: &Path,
    cfg: &Config,
    replace_foreign: bool,
) -> Result<registry::ProtocolRegistration> {
    let target = crate::versions::resolve_launch_target(root, cfg.use_current_junction, None)?;
    if replace_foreign {
        registry::replace_codex_protocol(cfg.install_mode, root, &target.executable)
    } else {
        registry::register_codex_protocol(cfg.install_mode, root, &target.executable)
    }
}

pub fn disable_codex_protocol(root: &Path, cfg: &Config) -> Result<registry::ProtocolRemoval> {
    registry::remove_codex_protocol_if_owned(cfg.install_mode, root)
}

fn report_protocol_sync_error(operation: &str, error: &anyhow::Error) {
    eprintln!("warn: codex protocol sync after {operation}: {error:#}");
    dialogs::error(&format!(
        "桌面应用{operation}已完成，但 codex:// 会话链接配置失败：{error:#}\n\n可稍后在中文助手的设置页重试。"
    ));
}

fn run_post_update_work(work: PostUpdateWork) {
    match start_menu_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) = write_start_menu_shortcut(&work.root, work.mode) {
                eprintln!("warn: shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: start menu shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
            ));
        }
    }
    match desktop_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) =
                create_or_update_desktop_shortcut(&work.root, work.mode, work.use_current_junction)
            {
                eprintln!("warn: desktop shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新 ChatGPT 桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: desktop shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查 ChatGPT 桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
            ));
        }
    }
    match assistant_desktop_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) = create_or_update_assistant_desktop_shortcut(&work.root, work.mode) {
                eprintln!("warn: assistant desktop shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新中文助手桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: assistant desktop shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查中文助手桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
            ));
        }
    }
    if work.register_uninstall {
        if let Err(e) = write_registry(&work.root, work.mode, &work.version) {
            eprintln!("warn: registry refresh: {e:#}");
        }
    }

    prune_versions(&work.root, work.keep_versions, work.keep_all_versions);
    let _ = std::fs::remove_file(&work.msix_path);
}

fn prune_versions(root: &Path, keep_versions: u32, keep_all_versions: bool) {
    let policy = if keep_all_versions {
        crate::versions::RetentionPolicy::KeepAll
    } else {
        crate::versions::RetentionPolicy::KeepLatest(keep_versions)
    };
    let running = crate::proxy::running_versions(&root.join("versions"));
    if let Err(error) = crate::versions::prune_installed(root, policy, &running) {
        eprintln!("warn: version pruning failed: {error:#}");
    }
}

/// Copy the currently-running launcher to `<root>/codex-launcher.exe`.
/// Must be a no-op if source == dest (e.g. someone re-runs the installer
/// from inside the install directory to reconfigure).
fn place_launcher(root: &Path) -> Result<()> {
    let src = std::env::current_exe().context("current_exe()")?;
    let dest = root.join("codex-launcher.exe");
    if same_file(&src, &dest) {
        // We're already running from the install location — nothing to do.
        return Ok(());
    }
    std::fs::copy(&src, &dest)
        .with_context(|| format!("copying launcher {} → {}", src.display(), dest.display()))?;
    Ok(())
}

/// Best-effort same-file check that tolerates differing path spellings
/// (UNC vs drive-letter, `..`, short names). Falls back to direct path
/// equality if either side can't be canonicalized (e.g. dest doesn't exist).
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// (Re)create the Start Menu `.lnk` with the launcher's embedded icon.
/// No-op for Portable mode (link_path returns None).
fn write_start_menu_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let Some(link) = shortcut::link_path(mode)? else {
        return Ok(());
    };
    let legacy_link = shortcut::legacy_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "")? {
        anyhow::bail!(
            "开始菜单已存在其他来源的 Codex Windows 中文助手快捷方式，为避免覆盖已保留原文件"
        );
    }
    let remove_legacy = match legacy_link.as_deref() {
        Some(path) => shortcut::is_owned(path, &target, "")?,
        None => false,
    };
    let icon = target.clone();
    shortcut::create_or_update(&link, &target, &icon, "Codex Windows 中文助手", root, "")?;
    if remove_legacy {
        if let Some(legacy_link) = legacy_link {
            shortcut::remove(&legacy_link)?;
        }
    }
    Ok(())
}

pub fn start_menu_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::link_path(mode)?,
        shortcut::legacy_link_path(mode)?,
    ]
    .into_iter()
    .flatten()
    {
        if shortcut::is_owned(&link, &target, "")? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remove_start_menu_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::link_path(mode)?,
        shortcut::legacy_link_path(mode)?,
    ]
    .into_iter()
    .flatten()
    {
        if shortcut::is_owned(&link, &target, "")? {
            shortcut::remove(&link)?;
        }
    }
    Ok(())
}

pub fn create_or_update_desktop_shortcut(
    root: &Path,
    mode: InstallMode,
    use_current_junction: bool,
) -> Result<()> {
    let link = shortcut::desktop_link_path(mode)?;
    let legacy_link = shortcut::legacy_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "--launch-latest")? {
        anyhow::bail!("桌面已存在其他来源的 ChatGPT 快捷方式，为避免覆盖已保留原文件");
    }
    let remove_legacy = shortcut::is_owned(&legacy_link, &target, "--launch-latest")?;
    let launch_target = crate::versions::resolve_launch_target(root, use_current_junction, None)?;
    let description = format!("启动最新 {}", launch_target.app_kind.display_name());
    shortcut::create_or_update(
        &link,
        &target,
        &launch_target.executable,
        &description,
        root,
        "--launch-latest",
    )?;
    if remove_legacy {
        shortcut::remove(&legacy_link)?;
    }
    Ok(())
}

pub fn desktop_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::desktop_link_path(mode)?,
        shortcut::legacy_desktop_link_path(mode)?,
    ] {
        if shortcut::is_owned(&link, &target, "--launch-latest")? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remove_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::desktop_link_path(mode)?,
        shortcut::legacy_desktop_link_path(mode)?,
    ] {
        if shortcut::is_owned(&link, &target, "--launch-latest")? {
            shortcut::remove(&link)?;
        }
    }
    Ok(())
}

pub fn create_or_update_assistant_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let link = shortcut::assistant_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "")? {
        anyhow::bail!(
            "桌面已存在其他来源的 Codex Windows 中文助手快捷方式，为避免覆盖已保留原文件"
        );
    }
    shortcut::create_or_update(
        &link,
        &target,
        &target,
        "打开 Codex Windows 中文助手",
        root,
        "",
    )
}

pub fn assistant_desktop_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    shortcut::is_owned(&shortcut::assistant_desktop_link_path(mode)?, &target, "")
}

pub fn remove_assistant_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let link = shortcut::assistant_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if shortcut::is_owned(&link, &target, "")? {
        shortcut::remove(&link)?;
    }
    Ok(())
}

/// (Re)write the Add/Remove Programs registry entry for the current install.
fn write_registry(root: &Path, mode: InstallMode, version: &str) -> Result<()> {
    let launcher = root.join("codex-launcher.exe");
    let entry = registry::UninstallEntry {
        display_name: "Codex Windows 中文助手",
        display_version: version,
        publisher: "chrichuang218",
        install_location: root,
        uninstall_string: format!("\"{}\" --uninstall", launcher.display()),
        display_icon: &launcher,
    };
    registry::write(mode, &entry)
}

fn emit_progress_detail(
    on_msg: &dyn Fn(InstallMsg),
    phase: &str,
    detail: &str,
    progress: Option<f32>,
) {
    on_msg(InstallMsg::ProgressDetail {
        phase: phase.into(),
        detail: detail.into(),
        progress,
    });
}

fn download_start_detail(fetcher: Fetcher) -> String {
    match fetcher {
        Fetcher::Direct => "正在解析 Codex 下载地址；如果直连超时，将自动切换 winget。".into(),
        Fetcher::Winget => "正在调用 winget 下载 Codex。".into(),
        Fetcher::LocalFile => "正在复制本地 MSIX 文件。".into(),
    }
}

fn update_start_detail(fetcher: Fetcher) -> String {
    match fetcher {
        Fetcher::Direct => "正在解析 Codex 下载地址；如果直连失败，将自动切换 winget。".into(),
        Fetcher::Winget => "正在调用 winget 下载 Codex；如果 winget 失败，将自动切换直连。".into(),
        other => download_start_detail(other),
    }
}

fn configured_update_fetcher(fetcher: Fetcher) -> Fetcher {
    match fetcher {
        Fetcher::LocalFile => Fetcher::Direct,
        other => other,
    }
}

/// Helper to pass a `&dyn Fn(InstallMsg)` into a `FnMut(u64, Option<u64>)`
/// progress callback without cloning the closure.
struct BoxedFn<'a>(&'a dyn Fn(InstallMsg), Option<&'a AtomicBool>);

impl BoxedFn<'_> {
    fn progress_bytes(&self, done: u64, total: Option<u64>) -> Result<()> {
        check_cancel(self.1)?;
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{} / {} MB", done / 1_048_576, t / 1_048_576),
            None => format!("{} MB", done / 1_048_576),
        };
        (self.0)(InstallMsg::ProgressDetail {
            phase: "Downloading".into(),
            detail,
            progress: frac,
        });
        Ok(())
    }

    fn progress_entries(&self, done: u64, total: Option<u64>) -> Result<()> {
        check_cancel(self.1)?;
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{done} / {t} files"),
            None => format!("{done} files"),
        };
        (self.0)(InstallMsg::ProgressDetail {
            phase: "Extracting".into(),
            detail,
            progress: frac,
        });
        Ok(())
    }
}

fn check_cancel(cancel: Option<&AtomicBool>) -> Result<()> {
    if cancel
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
    {
        anyhow::bail!("安装已取消");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol_test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("codex-windows-cn-{name}-{}", std::process::id()))
    }

    #[test]
    fn launch_surface_repairs_current_before_protocol_sync() {
        let root = protocol_test_root("protocol-current");
        let version = root.join("versions").join("2.0.0");
        let handler = version.join("ChatGPT.exe");
        let _ = junction::remove(&root.join("versions").join("current"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&version).expect("create version fixture");
        std::fs::write(&handler, b"handler").expect("write version fixture");

        sync_launch_surface_with(&root, true, |resolved| {
            assert_eq!(
                resolved,
                root.join("versions").join("current").join("ChatGPT.exe")
            );
            assert!(resolved.is_file(), "current target must exist before sync");
            Ok(())
        })
        .expect("sync launch surface");

        let _ = junction::remove(&root.join("versions").join("current"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn launch_surface_without_junction_uses_concrete_latest_handler() {
        let root = protocol_test_root("protocol-concrete");
        let old = root.join("versions").join("1.0.0");
        let latest = root.join("versions").join("2.0.0");
        let handler = latest.join("ChatGPT.exe");
        let _ = junction::remove(&root.join("versions").join("current"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&old).expect("create old version fixture");
        std::fs::create_dir_all(&latest).expect("create latest version fixture");
        std::fs::write(old.join("ChatGPT.exe"), b"old handler").expect("write old version fixture");
        std::fs::write(&handler, b"latest handler").expect("write latest version fixture");
        junction::set_current(&root, "1.0.0").expect("create stale current junction");

        sync_launch_surface_with(&root, false, |resolved| {
            assert_eq!(resolved, handler);
            assert!(!root.join("versions").join("current").exists());
            Ok(())
        })
        .expect("sync concrete launch surface");

        let _ = junction::remove(&root.join("versions").join("current"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn update_keeps_configured_direct_fetcher() {
        assert_eq!(configured_update_fetcher(Fetcher::Direct), Fetcher::Direct);
    }

    #[test]
    fn update_keeps_configured_winget_fetcher() {
        assert_eq!(configured_update_fetcher(Fetcher::Winget), Fetcher::Winget);
    }

    #[test]
    fn update_normalizes_legacy_local_file_fetcher_to_direct() {
        assert_eq!(
            configured_update_fetcher(Fetcher::LocalFile),
            Fetcher::Direct
        );
    }

    #[test]
    fn update_direct_detail_matches_actual_preferred_fetcher() {
        assert_eq!(
            update_start_detail(configured_update_fetcher(Fetcher::Direct)),
            "正在解析 Codex 下载地址；如果直连失败，将自动切换 winget。"
        );
    }
}
