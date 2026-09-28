use anyhow::{Context, Result};
use codex_windows_cn::{extract, store};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        !args.is_empty(),
        "usage: check_msix_runtime ROOT [MSIX VERSION]"
    );
    let root = PathBuf::from(&args[0]);
    let (archive, version) = if args.len() == 3 {
        (
            PathBuf::from(&args[1]),
            args[2].to_string_lossy().into_owned(),
        )
    } else {
        anyhow::ensure!(args.len() == 1, "expected ROOT or ROOT MSIX VERSION");
        let mut last = std::time::Instant::now();
        let result = store::download_latest(
            store::Fetcher::Direct,
            store::PRODUCT_ID_CODEX,
            &root.join("downloads"),
            &mut |done, total| {
                if last.elapsed().as_secs() >= 10 {
                    println!("download: {done} / {total:?}");
                    last = std::time::Instant::now();
                }
                Ok(())
            },
        )?;
        (result.msix_path, result.version)
    };
    println!("archive: {}\nversion: {version}", archive.display());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive)?)?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if entry.name().contains("oai/sky/package.json") {
            println!("archive entry: {}", entry.name());
        }
    }
    anyhow::ensure!(
        !root.join("versions").join(&version).exists(),
        "use a fresh test root"
    );
    let output = extract::extract_app(&archive, &root, &version, &mut |_, _| Ok(()))?;
    let bin = output.join("resources/cua_node/bin");
    for name in ["@oai", "%40oai"] {
        println!(
            "{name}/sky: {}",
            bin.join("node_modules")
                .join(name)
                .join("sky/package.json")
                .is_file()
        );
    }
    let status = std::process::Command::new(bin.join("node.exe"))
        .current_dir(&bin)
        .args(["--input-type=module", "-e", "const m = await import('@oai/sky'); console.log('sky export:', !!m.sky); console.log('service:', import.meta.resolve('@oai/sky/service'));"])
        .status().context("run extracted Node")?;
    anyhow::ensure!(status.success(), "extracted runtime module import failed");
    Ok(())
}
