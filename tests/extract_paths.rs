use codex_windows_cn::extract::extract_app;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "codex-extract-paths-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn archive(&self, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join("fixture.msix");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, content) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn extract(&self, archive: &Path, version: &str) -> anyhow::Result<PathBuf> {
        extract_app(archive, &self.0, version, &mut |_, _| Ok(()))
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn install_and_update_restore_msix_encoded_paths_once() {
    let root = TestRoot::new();
    let archive = root.archive(&[
        ("app/Codex.exe", b"fixture"),
        ("app/resources/node_modules/%40oai/sky/package.json", b"sky"),
        ("app/resources/node_modules/lib/%24value.js", b"dollar"),
        ("app/resources/%E4%B8%AD%E6%96%87.txt", b"unicode"),
        ("app/resources/%2540literal.txt", b"percent"),
        ("app/resources/a+b.txt", b"plus"),
    ]);
    for version in ["1.0.0", "2.0.0"] {
        let output = root.extract(&archive, version).unwrap();
        for (path, expected) in [
            ("node_modules/@oai/sky/package.json", "sky"),
            ("node_modules/lib/$value.js", "dollar"),
            ("中文.txt", "unicode"),
            ("%40literal.txt", "percent"),
            ("a+b.txt", "plus"),
        ] {
            assert_eq!(
                std::fs::read_to_string(output.join("resources").join(path)).unwrap(),
                expected
            );
        }
        assert!(!output.join("resources/node_modules/%40oai").exists());
    }
}

#[test]
fn rejects_unsafe_paths_after_decoding() {
    for path in [
        "%2e%2e/escaped.txt",
        "%2e%2e%5cescaped.txt",
        "%2e%2e%20/escaped.txt",
        "%2fabsolute.txt",
        "C%3a/escaped.txt",
        "file%3astream",
        "bad%00name",
        "%FF.txt",
        "../escaped.txt",
        "/absolute.txt",
    ] {
        let root = TestRoot::new();
        let entry = format!("app/{path}");
        let archive = root.archive(&[("app/Codex.exe", b"fixture"), (&entry, b"bad")]);
        assert!(root.extract(&archive, "1.0.0").is_err(), "accepted {path}");
        assert!(!root.0.join("versions/1.0.0").exists());
    }
}

#[test]
fn decoded_name_collision_does_not_overwrite_a_file() {
    let root = TestRoot::new();
    let archive = root.archive(&[
        ("app/Codex.exe", b"fixture"),
        ("app/resources/%40oai/package.json", b"first"),
        ("app/resources/@oai/package.json", b"second"),
    ]);
    assert!(root.extract(&archive, "1.0.0").is_err());
    assert_eq!(
        std::fs::read(
            root.0
                .join("versions/1.0.0.partial/resources/@oai/package.json")
        )
        .unwrap(),
        b"first"
    );
}
