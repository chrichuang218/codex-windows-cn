//! Progress handoff for the one-shot elevated installer/update helper.
//! Complete JSON lines are visible to the parent before the helper exits.

use crate::installer::InstallMsg;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

fn progress_path(result_path: &Path) -> PathBuf {
    result_path.with_extension("progress.jsonl")
}

pub struct ProgressReader {
    path: PathBuf,
    file: Option<File>,
    pending: Vec<u8>,
}

impl ProgressReader {
    pub fn create(result_path: &Path) -> Result<Self> {
        let path = progress_path(result_path);
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .context("创建管理员任务进度文件失败")?;
        Ok(Self {
            path,
            file: Some(file),
            pending: Vec::new(),
        })
    }

    pub fn read_latest(&mut self) -> Result<Option<InstallMsg>> {
        self.file
            .as_mut()
            .expect("progress reader is open")
            .read_to_end(&mut self.pending)
            .context("读取管理员任务进度失败")?;
        let mut latest = None;
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            let msg: InstallMsg =
                serde_json::from_slice(&line).context("管理员任务进度格式无效")?;
            // Only the process exit and its result file may finish an operation.
            anyhow::ensure!(
                !matches!(msg, InstallMsg::Done { .. } | InstallMsg::Error(_)),
                "管理员任务进度中包含了意外的最终结果"
            );
            latest = Some(msg);
        }
        Ok(latest)
    }
}

impl Drop for ProgressReader {
    fn drop(&mut self) {
        self.file.take();
        // This is a disposable progress file; the operation result is separate.
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct ProgressWriter {
    file: File,
    last_sent: Option<Instant>,
    phase: Option<String>,
}

impl ProgressWriter {
    pub fn open(result_path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .open(progress_path(result_path))
            .context("打开管理员任务进度文件失败")?;
        Ok(Self {
            file,
            last_sent: None,
            phase: None,
        })
    }

    pub fn publish(&mut self, msg: InstallMsg) -> Result<()> {
        let (phase, progress, always_send) = match &msg {
            InstallMsg::ProgressDetail {
                phase, progress, ..
            } => (
                Some(phase.as_str()),
                *progress,
                self.phase.as_deref() != Some(phase.as_str()),
            ),
            InstallMsg::Phase { phase, .. } => (Some(phase.as_str()), None, true),
            InstallMsg::Progress(progress) => (self.phase.as_deref(), *progress, false),
            InstallMsg::Done { .. } | InstallMsg::Error(_) => {
                anyhow::bail!("最终结果必须通过管理员任务结果文件返回")
            }
        };
        let boundary = progress.map_or(true, |value| value <= 0.0 || value >= 1.0);
        if !always_send
            && !boundary
            && self
                .last_sent
                .is_some_and(|last| last.elapsed() < PROGRESS_INTERVAL)
        {
            return Ok(());
        }
        let mut line = serde_json::to_vec(&msg).context("序列化管理员任务进度失败")?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .context("回传管理员任务进度失败")?;
        self.last_sent = Some(Instant::now());
        self.phase = phase.map(str::to_owned);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn result_path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "codex-progress-test-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn progress(phase: &str, detail: &str, value: f32) -> InstallMsg {
        InstallMsg::ProgressDetail {
            phase: phase.into(),
            detail: detail.into(),
            progress: Some(value),
        }
    }

    #[test]
    fn complete_progress_is_visible_and_partial_utf8_waits_for_a_newline() {
        let result = result_path();
        let mut reader = ProgressReader::create(&result).unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(progress_path(&result))
            .unwrap();
        let msg = progress("Downloading", "第 2/3 次尝试", 0.0);
        let mut raw = serde_json::to_vec(&msg).unwrap();
        raw.push(b'\n');
        let split = raw.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
        file.write_all(&raw[..split]).unwrap();
        assert!(reader.read_latest().unwrap().is_none());
        file.write_all(&raw[split..]).unwrap();
        assert!(matches!(reader.read_latest().unwrap(),
            Some(InstallMsg::ProgressDetail { detail, progress: Some(0.0), .. })
            if detail == "第 2/3 次尝试"
        ));
        assert!(reader.read_latest().unwrap().is_none());
        drop(file);
        drop(reader);
        assert!(!progress_path(&result).exists());
    }

    #[test]
    fn retry_resets_and_phase_changes_are_not_throttled() {
        let result = result_path();
        let mut reader = ProgressReader::create(&result).unwrap();
        let mut writer = ProgressWriter::open(&result).unwrap();
        writer
            .publish(progress("Downloading", "第 1/3 次尝试", 0.6))
            .unwrap();
        reader.read_latest().unwrap();
        writer
            .publish(progress("Downloading", "第 1/3 次尝试", 0.7))
            .unwrap();
        assert!(reader.read_latest().unwrap().is_none());
        writer
            .publish(progress("Downloading", "第 2/3 次尝试", 0.0))
            .unwrap();
        assert!(matches!(reader.read_latest().unwrap(),
            Some(InstallMsg::ProgressDetail { detail, progress: Some(0.0), .. })
            if detail == "第 2/3 次尝试"
        ));
        writer
            .publish(progress("Extracting", "正在解压", 0.5))
            .unwrap();
        assert!(matches!(reader.read_latest().unwrap(),
            Some(InstallMsg::ProgressDetail { phase, .. }) if phase == "Extracting"
        ));
    }

    #[test]
    fn invalid_progress_cannot_publish_a_fake_terminal_result() {
        let result = result_path();
        let mut reader = ProgressReader::create(&result).unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(progress_path(&result))
            .unwrap();
        let mut raw = serde_json::to_vec(&InstallMsg::Done {
            version: "fake".into(),
        })
        .unwrap();
        raw.push(b'\n');
        file.write_all(&raw).unwrap();
        assert!(reader
            .read_latest()
            .unwrap_err()
            .to_string()
            .contains("意外的最终结果"));
        assert!(ProgressWriter::open(&result)
            .unwrap()
            .publish(InstallMsg::Error("fake".into()))
            .is_err());
    }
}
