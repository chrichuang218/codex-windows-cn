//! Local-file "fetcher": user already has the MSIX on disk.
//!
//! This is not really a fetcher — nothing is fetched. Given a path to a
//! pre-downloaded `.msix`/`.msixbundle`, we validate it, copy it into the
//! destination directory (so downstream extraction code sees the same
//! shape as the Direct/Winget paths), and synthesize a `DownloadResult`
//! with version/moniker parsed from the filename.
//!
//! Intended as a manual escape hatch for users behind restrictive
//! networks who obtained the MSIX from elsewhere (a different machine,
//! a USB stick, rg-adguard, etc.). NOT part of the auto-fallback chain
//! — selecting this fetcher always requires an explicit `--msix <path>`.

use super::DownloadResult;
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};

/// Copy `src` into `dest_dir` and return a DownloadResult describing it.
/// Version/moniker are parsed from the filename, which for a genuine Store
/// MSIX looks like `OpenAI.Codex_26.422.2437.0_x64__2p2nqsd0c76g0.msix`.
pub fn from_file(
    src: &Path,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<DownloadResult> {
    if !src.exists() {
        bail!("MSIX file does not exist: {}", src.display());
    }
    let ext_ok = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("msix") || s.eq_ignore_ascii_case("msixbundle"))
        .unwrap_or(false);
    if !ext_ok {
        bail!("file is not an .msix or .msixbundle: {}", src.display());
    }

    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("filename is not valid UTF-8: {}", src.display()))?
        .to_string();

    let version = stem
        .split('_')
        .nth(1)
        .ok_or_else(|| {
            anyhow!(
                "filename doesn't look like a Store MSIX moniker \
                 (expected 'Publisher.App_version_arch__id'): {}",
                stem
            )
        })?
        .to_string();

    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("creating {}", dest_dir.display()))?;
    let dest: PathBuf = dest_dir.join(
        src.file_name()
            .ok_or_else(|| anyhow!("source path has no filename"))?,
    );

    // Honor progress for the copy too — lets the UI show activity even
    // when "downloading" is really just a local file copy.
    copy_with_progress(src, &dest, progress)
        .with_context(|| format!("copying to {}", dest.display()))?;

    Ok(DownloadResult {
        msix_path: dest,
        moniker: stem,
        version,
    })
}

fn copy_with_progress(
    src: &Path,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<()> {
    use std::io::{Read, Write};
    let total = std::fs::metadata(src).ok().map(|m| m.len());
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::File::create(dest)?;
    let mut buf = [0u8; 256 * 1024];
    let mut written = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        written += n as u64;
        progress(written, total)?;
    }
    output.flush()?;
    Ok(())
}
