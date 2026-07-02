//! Update check + defer-decision helpers.
//!
//! The update check is cheap: it resolves the Store's latest version string
//! via FE3 SyncUpdates (no download) and compares against `config.current_version`.
//! Callers apply the user's response by mutating the Config and saving it.
//!
//! Decision flow:
//!   1. Policy == Never          → Skipped
//!   2. suppress_until_unix > now → Skipped
//!   3. Resolve latest
//!      - err                     → Error (show but let user continue)
//!      - == current              → UpToDate
//!      - != current              → Available { current, latest }
//!
//! The policy (Always/Daily/Weekly) only gates *automatic* checks from proxy
//! mode — an explicit "Check for updates" action bypasses it.

use crate::config::{Config, UpdatePolicy};
use crate::store::{self, Fetcher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Owner/repo for the launcher's own GitHub releases. Tweak here if the
/// project ever moves.
pub const LAUNCHER_REPO: &str = "chrichuang218/codex-windows-cn";
pub const LAUNCHER_LATEST_API: &str =
    "https://api.github.com/repos/chrichuang218/codex-windows-cn/releases/latest";

#[derive(Debug, Clone)]
pub enum UpdateDecision {
    /// Skip the check entirely (policy=Never, suppressed, or policy cooldown).
    Skipped { reason: String },
    /// Check ran; we're on the latest version.
    UpToDate { version: String },
    /// Check ran; a newer version exists.
    Available { current: String, latest: String },
    /// Check failed — surface the error but don't block the app.
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferChoice {
    /// Update right now.
    UpdateNow,
    /// Remind me on the next scheduled check.
    NotNow,
    /// Skip this specific version forever.
    SkipThisVersion,
    /// Snooze 1 day.
    SnoozeOneDay,
    /// Snooze 7 days.
    SnoozeSevenDays,
    /// Turn off all update prompts.
    Never,
}

/// Automatic check — honors policy cooldown + suppress_until. Use this from
/// proxy-mode startup.
pub fn check_auto(cfg: &Config, product_id: &str) -> UpdateDecision {
    if let Some(reason) = auto_check_skip_reason(cfg) {
        return UpdateDecision::Skipped { reason };
    }
    let decision = check_now(cfg, product_id);
    // Honor "skip this version" only while the Store's latest still matches
    // the skipped version — once Microsoft publishes something newer, the
    // suppression is implicitly lifted.
    if let UpdateDecision::Available { latest, .. } = &decision {
        if cfg.skipped_version.as_deref() == Some(latest.as_str()) {
            return UpdateDecision::Skipped {
                reason: format!("version {latest} skipped by user"),
            };
        }
    }
    decision
}

/// True when `check_auto` will perform the Store version lookup instead of
/// skipping immediately due to policy, snooze, or cooldown.
pub fn auto_check_will_query(cfg: &Config) -> bool {
    auto_check_skip_reason(cfg).is_none()
}

/// Force a check regardless of policy/suppression. Use this when the user
/// explicitly clicks "Check for updates".
pub fn check_now(cfg: &Config, product_id: &str) -> UpdateDecision {
    match store::resolve_latest_version(cfg.fetcher, product_id) {
        Ok(latest) => {
            if version_gt(&latest, &cfg.current_version) {
                UpdateDecision::Available {
                    current: cfg.current_version.clone(),
                    latest,
                }
            } else {
                UpdateDecision::UpToDate { version: latest }
            }
        }
        Err(e) => UpdateDecision::Error(format!("{:#}", e)),
    }
}

/// Apply a defer choice to `cfg`. Caller is responsible for saving afterward.
/// `latest` is the version the user was prompted about (used for SkipThisVersion).
pub fn apply_defer(cfg: &mut Config, choice: DeferChoice, latest: &str) {
    let now = now_unix();
    cfg.last_check_unix = Some(now);
    cfg.known_latest = Some(latest.to_string());
    match choice {
        DeferChoice::UpdateNow => {
            cfg.suppress_until_unix = None;
            cfg.skipped_version = None;
        }
        DeferChoice::NotNow => {
            // Let normal policy cooldown govern the next check — nothing to do.
        }
        DeferChoice::SkipThisVersion => {
            // Suppress prompts specifically for this version. `check_auto`
            // filters Available decisions where latest == skipped_version;
            // once the Store moves past this version, prompts resume.
            cfg.skipped_version = Some(latest.to_string());
        }
        DeferChoice::SnoozeOneDay => cfg.suppress_until_unix = Some(now + 86_400),
        DeferChoice::SnoozeSevenDays => cfg.suppress_until_unix = Some(now + 7 * 86_400),
        DeferChoice::Never => {
            cfg.update_policy = UpdatePolicy::Never;
            cfg.suppress_until_unix = None;
        }
    }
}

/// Record a successful up-to-date check — bumps `last_check_unix` + known_latest.
pub fn record_check(cfg: &mut Config, latest: &str) {
    cfg.last_check_unix = Some(now_unix());
    cfg.known_latest = Some(latest.to_string());
}

// -- Launcher self-update check -------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)] // Skipped/Error fields are reserved for future logging.
pub enum LauncherDecision {
    /// Skipped (policy=Never, snoozed, within cooldown, or skip-this-version match).
    Skipped { reason: String },
    /// We're on the latest tag.
    UpToDate { version: String },
    /// A newer tag is published. `release_url` is the GitHub Releases page.
    Available {
        current: String,
        latest: String,
        release_url: String,
    },
    /// API call or parse failed. Surface but don't block.
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherDeferChoice {
    /// Open the release page in the user's default browser. Doesn't dismiss.
    ViewRelease,
    /// Defer until next cooldown roll.
    NotNow,
    /// Suppress prompts only while GitHub's latest equals this version.
    SkipThisVersion,
    SnoozeOneDay,
    SnoozeSevenDays,
    /// Suppress launcher prompts effectively forever (suppress_until = u64::MAX).
    Never,
    /// Download the new launcher and replace the running exe in place.
    /// Caller drives the download/swap; this variant is just the action signal.
    ApplyUpdate,
}

/// Reconstruct a pending launcher prompt from persisted state — no network.
/// Used by paths that can't re-run the bg check (e.g. elevated `--auto-update`
/// re-spawn — the unelevated process already bumped the shared cooldown).
/// Returns `Some` only if `known_latest_launcher` is newer than the running
/// version and the user hasn't silenced it (skip / snooze / never).
pub fn pending_launcher_from_state(cfg: &Config) -> Option<LauncherDecision> {
    if cfg.update_policy == UpdatePolicy::Never {
        return None;
    }
    if let Some(until) = cfg.launcher_suppress_until_unix {
        if now_unix() < until {
            return None;
        }
    }
    let latest = cfg.known_latest_launcher.as_ref()?;
    if cfg.skipped_launcher_version.as_deref() == Some(latest.as_str()) {
        return None;
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    if !version_gt(latest, &current) {
        return None;
    }
    Some(LauncherDecision::Available {
        current,
        latest: latest.clone(),
        release_url: format!("https://github.com/{LAUNCHER_REPO}/releases/tag/v{latest}"),
    })
}

/// Automatic launcher-update check. Honors the shared `update_policy` and
/// `last_check_unix` cooldown, plus launcher-specific snooze and
/// skip-this-version. Doesn't itself update `last_check_unix` — caller is
/// expected to record after both checks complete.
pub fn check_launcher_auto(cfg: &Config) -> LauncherDecision {
    if let Some(reason) = launcher_auto_check_skip_reason(cfg) {
        return LauncherDecision::Skipped { reason };
    }

    let decision = check_launcher_now();
    if let LauncherDecision::Available { latest, .. } = &decision {
        if cfg.skipped_launcher_version.as_deref() == Some(latest.as_str()) {
            return LauncherDecision::Skipped {
                reason: format!("launcher version {latest} skipped by user"),
            };
        }
    }
    decision
}

/// True when `check_launcher_auto` will call GitHub instead of skipping
/// immediately due to policy, launcher snooze, or shared cooldown.
pub fn launcher_auto_check_will_query(cfg: &Config) -> bool {
    launcher_auto_check_skip_reason(cfg).is_none()
}

/// Force a launcher-update check regardless of policy/snooze/cooldown.
pub fn check_launcher_now() -> LauncherDecision {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_latest_launcher_tag() {
        Ok(tag) => {
            // Strip "v" prefix if present (we tag releases as v0.1.0 but
            // CARGO_PKG_VERSION is plain 0.1.0).
            let latest_ver = tag.trim_start_matches('v').to_string();
            if version_gt(&latest_ver, &current) {
                LauncherDecision::Available {
                    current,
                    latest: latest_ver,
                    release_url: format!("https://github.com/{LAUNCHER_REPO}/releases/tag/{tag}"),
                }
            } else {
                LauncherDecision::UpToDate {
                    version: latest_ver,
                }
            }
        }
        Err(e) => LauncherDecision::Error(format!("{e:#}")),
    }
}

fn fetch_latest_launcher_tag() -> anyhow::Result<String> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct LatestRelease {
        tag_name: String,
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("codex-windows-updater/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .get(LAUNCHER_LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?;
    let body: LatestRelease = resp.json()?;
    Ok(body.tag_name)
}

/// Apply a launcher defer choice. Caller persists.
pub fn apply_launcher_defer(cfg: &mut Config, choice: LauncherDeferChoice, latest: &str) {
    let now = now_unix();
    cfg.known_latest_launcher = Some(latest.to_string());
    match choice {
        LauncherDeferChoice::ViewRelease
        | LauncherDeferChoice::NotNow
        | LauncherDeferChoice::ApplyUpdate => {
            // No state change beyond known_latest_launcher. Cooldown
            // governs next prompt. ApplyUpdate is a no-op here — the
            // self-update worker mutates state on success.
        }
        LauncherDeferChoice::SkipThisVersion => {
            cfg.skipped_launcher_version = Some(latest.to_string());
        }
        LauncherDeferChoice::SnoozeOneDay => {
            cfg.launcher_suppress_until_unix = Some(now + 86_400);
        }
        LauncherDeferChoice::SnoozeSevenDays => {
            cfg.launcher_suppress_until_unix = Some(now + 7 * 86_400);
        }
        LauncherDeferChoice::Never => {
            cfg.launcher_suppress_until_unix = Some(u64::MAX);
        }
    }
}

fn policy_cooldown_secs(p: UpdatePolicy) -> u64 {
    match p {
        UpdatePolicy::Always => 0,
        UpdatePolicy::Daily => 86_400,
        UpdatePolicy::Weekly => 7 * 86_400,
        UpdatePolicy::Never => u64::MAX, // unreachable (filtered earlier)
    }
}

fn auto_check_skip_reason(cfg: &Config) -> Option<String> {
    let now = now_unix();
    if cfg.update_policy == UpdatePolicy::Never {
        return Some("update_policy = never".into());
    }
    if let Some(until) = cfg.suppress_until_unix {
        if now < until {
            let days = (until - now) / 86_400;
            return Some(format!("suppressed for ~{days}d"));
        }
    }
    if let Some(last) = cfg.last_check_unix {
        let cooldown = policy_cooldown_secs(cfg.update_policy);
        if now.saturating_sub(last) < cooldown {
            return Some("within cooldown".into());
        }
    }
    None
}

fn launcher_auto_check_skip_reason(cfg: &Config) -> Option<String> {
    let now = now_unix();
    if cfg.update_policy == UpdatePolicy::Never {
        return Some("update_policy = never".into());
    }
    if let Some(until) = cfg.launcher_suppress_until_unix {
        if now < until {
            let days = (until - now) / 86_400;
            return Some(format!("launcher prompt suppressed for ~{days}d"));
        }
    }
    if let Some(last) = cfg.last_check_unix {
        let cooldown = policy_cooldown_secs(cfg.update_policy);
        if now.saturating_sub(last) < cooldown {
            return Some("within cooldown".into());
        }
    }
    None
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Dotted-numeric compare. `a > b`?
fn version_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    pa > pb
}

// Keep this so the `Fetcher` import stays live if we later gate by it.
#[allow(dead_code)]
fn _fetcher_check(f: Fetcher) -> Fetcher {
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_gt("26.500.0.0", "26.422.2437.0"));
        assert!(!version_gt("26.422.2437.0", "26.422.2437.0"));
        assert!(!version_gt("26.100.0.0", "26.422.0.0"));
        assert!(version_gt("27.0.0.0", "26.999.999.999"));
    }
}
