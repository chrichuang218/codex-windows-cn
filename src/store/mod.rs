//! Microsoft Store MSIX fetcher.
//!
//! Two strategies:
//! - [`Fetcher::Direct`] — DisplayCatalog + FE3 SOAP, no local tool deps
//! - [`Fetcher::Winget`] — shells out to `winget.exe`
//!
//! Prefer Direct; use Winget as a fallback when the direct path is blocked
//! (firewall, custom root CA policy, etc.).

mod direct;
pub mod local_file;
mod winget;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PRODUCT_ID_CODEX: &str = "9PLM9XGG6VKS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Fetcher {
    #[default]
    Direct,
    Winget,
    /// Manual escape hatch — user provides a path to a pre-downloaded MSIX.
    /// NOT part of the auto-fallback chain; callers must invoke
    /// [`local_file::from_file`] directly with the path.
    LocalFile,
}

impl Fetcher {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "direct" => Some(Fetcher::Direct),
            "winget" => Some(Fetcher::Winget),
            "local" | "local_file" | "localfile" | "file" => Some(Fetcher::LocalFile),
            _ => None,
        }
    }

    /// The "other" network fetcher for auto-fallback. LocalFile isn't part
    /// of this chain — it requires explicit user intent + a file path.
    pub fn other(self) -> Self {
        match self {
            Fetcher::Direct => Fetcher::Winget,
            Fetcher::Winget => Fetcher::Direct,
            Fetcher::LocalFile => Fetcher::Direct,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// Absolute path to the downloaded `.msix` / `.msixbundle` on disk.
    pub msix_path: PathBuf,
    /// e.g. `OpenAI.Codex_26.422.2437.0_x64__2p2nqsd0c76g0`
    pub moniker: String,
    /// MSIX package version, e.g. `26.422.2437.0`.
    pub version: String,
}

/// Download the latest MSIX via the specified fetcher. Progress callback
/// fires periodically as bytes arrive; `total_opt` may be `None` when the
/// fetcher can't determine the full size (e.g. winget).
pub fn download_latest(
    fetcher: Fetcher,
    product_id: &str,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<DownloadResult> {
    match fetcher {
        Fetcher::Direct => direct::download_latest(product_id, dest_dir, progress),
        Fetcher::Winget => winget::download_latest(product_id, dest_dir, progress),
        Fetcher::LocalFile => Err(anyhow::anyhow!(
            "LocalFile fetcher requires an explicit file path — call \
             store::local_file::from_file directly, not download_latest()"
        )),
    }
}

/// Try `preferred`, and if it fails try the other strategy. Returns which
/// fetcher actually succeeded. Callers *may* persist that choice to
/// `updater.json`, but beware: a single transient failure (offline, proxy
/// blip) followed by the network recovering on the fallback attempt would
/// permanently demote the user's chosen fetcher. The installer currently
/// does not persist for that reason — see `installer.rs`. A stronger
/// signal (N consecutive failures) would be needed before flipping.
pub fn download_latest_with_fallback(
    preferred: Fetcher,
    product_id: &str,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(DownloadResult, Fetcher)> {
    match download_latest(preferred, product_id, dest_dir, progress) {
        Ok(r) => Ok((r, preferred)),
        Err(primary_err) => {
            let fallback = preferred.other();
            match download_latest(fallback, product_id, dest_dir, progress) {
                Ok(r) => Ok((r, fallback)),
                Err(fallback_err) => Err(anyhow::anyhow!(
                    "both fetchers failed\n  {:?}: {}\n  {:?}: {}",
                    preferred,
                    primary_err,
                    fallback,
                    fallback_err
                )),
            }
        }
    }
}

/// Resolve the latest available version string without downloading the MSIX.
/// Used by the update checker. Currently Direct-only — winget has no
/// cheap version-query path that doesn't also trigger a download under
/// the current Entra-auth gate.
pub fn resolve_latest_version(_fetcher: Fetcher, product_id: &str) -> Result<String> {
    direct::resolve_latest_version(product_id)
}

/// Debug helper: dump the raw (html-decoded) SyncUpdates SOAP response.
/// Direct-only — winget has no equivalent.
pub fn debug_dump_sync_xml(product_id: &str) -> Result<String> {
    direct::debug_dump_sync_xml(product_id)
}
