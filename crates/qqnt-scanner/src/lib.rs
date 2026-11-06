use cap_std::fs::Dir;
use chrono::{DateTime, Utc};
use qqnt_adapters::{is_link, reject_links};
use qqnt_classifier::{classify, protected_path};
use qqnt_model::{Detection, IndexedFile, ScanProgress, ScanSummary};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub const BATCH_SIZE: usize = 256;

/// Reads metadata and at most 32 header bytes per regular, unprotected file.
/// The sink is bounded; neither the scanner nor the IPC stores a complete file list.
pub fn scan(
    root: &Dir,
    detection: &Detection,
    cancel: &AtomicBool,
    progress: impl FnMut(&ScanProgress),
    sink: impl FnMut(&[IndexedFile]) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    scan_incremental(root, detection, cancel, |_, _| Ok(None), progress, sink)
}

pub fn scan_incremental(
    root: &Dir,
    detection: &Detection,
    cancel: &AtomicBool,
    mut cached: impl FnMut(&str, &qqnt_model::Fingerprint) -> io::Result<Option<IndexedFile>>,
    mut progress: impl FnMut(&ScanProgress),
    mut sink: impl FnMut(&[IndexedFile]) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    let start = Instant::now();
    let mut last_emit = start;
    let mut summary = ScanSummary::default();
    let mut stack = vec![PathBuf::new()];
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    while let Some(relative_dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !relative_dir.as_os_str().is_empty() && reject_links(root, &relative_dir).is_err() {
            summary.progress.skipped_links += 1;
            continue;
        }
        let entries = if relative_dir.as_os_str().is_empty() {
            root.entries()
        } else {
            root.read_dir(&relative_dir)
        };
        let entries = match entries {
            Ok(entries) => entries,
            Err(e) => {
                record_error(&mut summary, e.kind());
                continue;
            }
        };
        summary.progress.directories += 1;
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if last_emit.elapsed().as_millis() >= 100 {
                summary.progress.elapsed_ms = start.elapsed().as_millis() as u64;
                progress(&summary.progress);
                last_emit = Instant::now();
            }
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    record_error(&mut summary, e.kind());
                    continue;
                }
            };
            let path = relative_dir.join(entry.file_name());
            if relative_dir.as_os_str().is_empty()
                && entry.file_name() == ".qqnt-storage-manager-quarantine"
            {
                if !summary.warnings.iter().any(|w| w.contains("隔离区")) {
                    summary
                        .warnings
                        .push("隔离区未计入 QQ 文件统计，请在清理中心单独查看".into());
                }
                continue;
            }
            let meta = match root.symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    record_error(&mut summary, e.kind());
                    continue;
                }
            };
            if is_link(&meta) {
                summary.progress.skipped_links += 1;
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let Some(relative_path) = path.to_str().map(|s| s.replace('\\', "/")) else {
                record_error(&mut summary, io::ErrorKind::InvalidData);
                continue;
            };
            if reject_links(root, &path).is_err() {
                summary.progress.skipped_links += 1;
                continue;
            }
            let mut header = [0u8; 32];
            let mut read_len = 0;
            let mut unreadable = false;
            let mut fingerprint = None;
            let mut reused = None;
            match root.open(&path) {
                Ok(f) => {
                    let mut file = f.into_std();
                    fingerprint = qqnt_fs::fingerprint(&file).ok();
                    if let Some(fp) = &fingerprint {
                        reused = cached(&relative_path, fp)?;
                    }
                    if reused.is_some() {
                        summary.progress.reused_files += 1;
                    }
                    if reused.is_none() && !protected_path(&path) {
                        match file.read(&mut header) {
                            Ok(n) => read_len = n,
                            Err(e) => {
                                record_error(&mut summary, e.kind());
                                unreadable = true;
                            }
                        }
                    }
                }
                Err(e) if !protected_path(&path) => {
                    record_error(&mut summary, e.kind());
                    unreadable = true;
                }
                Err(_) => {}
            }
            let (kind, mut risk, mut evidence) = match reused {
                Some(f) => (f.kind, f.risk, f.evidence),
                None => classify(&path, &header[..read_len], detection.recognized),
            };
            if meta.permissions().readonly() {
                risk = qqnt_model::Risk::Protected;
                evidence.push("文件只读属性：按受保护数据处理".into());
            }
            if unreadable {
                evidence.push("文件头读取失败；已保留可用的元数据统计".into());
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.into_std().duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let created = meta
                .created()
                .ok()
                .and_then(|t| t.into_std().duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let directory = path
                .components()
                .next()
                .filter(|_| path.parent() != Some(Path::new("")))
                .map(|p| p.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_else(|| "(根目录)".into());
            let size = fingerprint.as_ref().map_or(meta.len(), |f| f.size);
            summary.progress.files += 1;
            summary.progress.bytes = summary.progress.bytes.saturating_add(size);
            summary.by_kind.entry(kind).or_default().add(size);
            summary.by_risk.entry(risk).or_default().add(size);
            summary
                .by_directory
                .entry(directory.clone())
                .or_default()
                .add(size);
            let month = modified
                .and_then(|t| DateTime::<Utc>::from_timestamp(t, 0))
                .map(|t| t.format("%Y-%m").to_string())
                .unwrap_or_else(|| "未知时间".into());
            summary.by_month.entry(month).or_default().add(size);
            batch.push(IndexedFile {
                relative_path,
                directory,
                size,
                created,
                modified,
                kind,
                risk,
                evidence,
                fingerprint,
                user_protected: false,
            });
            if batch.len() == BATCH_SIZE {
                sink(&batch)?;
                batch.clear();
            }
        }
    }
    if !batch.is_empty() {
        sink(&batch)?;
    }
    summary.cancelled = cancel.load(Ordering::Relaxed);
    summary.progress.elapsed_ms = start.elapsed().as_millis() as u64;
    progress(&summary.progress);
    tracing::info!(
        files = summary.progress.files,
        errors = summary.progress.errors,
        cancelled = summary.cancelled,
        "scan finished"
    );
    Ok(summary)
}

fn record_error(summary: &mut ScanSummary, kind: io::ErrorKind) {
    summary.progress.errors += 1;
    let message = format!("读取失败：{kind:?}（详细路径不写入日志）");
    if summary.warnings.len() < 20 && !summary.warnings.contains(&message) {
        summary.warnings.push(message);
    }
}
