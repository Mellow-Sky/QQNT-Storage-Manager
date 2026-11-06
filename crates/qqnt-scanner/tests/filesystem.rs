use qqnt_adapters::{open_authorized, QQNTAdapter, StructuralAdapter};
use qqnt_model::{IndexedFile, Risk, ScanSummary};
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn run(root: &Path) -> io::Result<(ScanSummary, Vec<IndexedFile>)> {
    let (_, dir) = open_authorized(root)?;
    let mut files = Vec::new();
    let summary = qqnt_scanner::scan(
        &dir,
        &StructuralAdapter.detect(&dir),
        &AtomicBool::new(false),
        |_| {},
        |batch| {
            files.extend_from_slice(batch);
            Ok(())
        },
    )?;
    Ok((summary, files))
}
fn layout(root: &Path) -> io::Result<()> {
    for name in ["Pic", "Video", "nt_db", "File"] {
        fs::create_dir_all(root.join(name))?;
    }
    Ok(())
}

#[test]
fn real_metadata_classification_unicode_and_no_writes() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    layout(temp.path())?;
    let media = temp.path().join("Pic/中文图片.dat");
    fs::write(&media, b"\x89PNG\r\n\x1a\ncontents")?;
    fs::write(temp.path().join("Video/a.mp4"), b"\0\0\0\x18ftypmp42rest")?;
    fs::write(
        temp.path().join("nt_db/message.db"),
        b"SQLite format 3\0private",
    )?;
    fs::write(temp.path().join("File/important.png"), b"\xff\xd8\xff")?;
    fs::write(temp.path().join("Pic/broken.jpg"), b"broken")?;
    let before = fs::metadata(&media)?.modified()?;
    let snapshot: Vec<_> = [
        "Pic/中文图片.dat",
        "Video/a.mp4",
        "nt_db/message.db",
        "File/important.png",
        "Pic/broken.jpg",
    ]
    .iter()
    .map(|p| fs::read(temp.path().join(p)))
    .collect::<io::Result<_>>()?;
    let (summary, files) = run(temp.path())?;
    assert_eq!(summary.progress.files, 5);
    assert_eq!(
        summary.progress.bytes,
        snapshot.iter().map(|v| v.len() as u64).sum::<u64>()
    );
    assert_eq!(summary.by_kind[&qqnt_model::MediaKind::Image].count, 3);
    assert_eq!(summary.by_risk[&Risk::Protected].count, 2);
    assert!(files
        .iter()
        .all(|f| !f.evidence.is_empty() && f.risk != Risk::SafeCandidate));
    assert_eq!(fs::metadata(&media)?.modified()?, before);
    for (name, bytes) in [
        "Pic/中文图片.dat",
        "Video/a.mp4",
        "nt_db/message.db",
        "File/important.png",
        "Pic/broken.jpg",
    ]
    .iter()
    .zip(snapshot)
    {
        assert_eq!(fs::read(temp.path().join(name))?, bytes);
    }
    Ok(())
}

#[test]
fn empty_unknown_and_long_paths() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    assert_eq!(run(temp.path())?.0.progress.files, 0);
    let long = (0..12).fold(temp.path().to_path_buf(), |p, i| {
        p.join(format!("长路径_component_{i:02}_abcdefghijk"))
    });
    fs::create_dir_all(&long)?;
    fs::write(long.join("photo.jpg"), b"\xff\xd8\xff")?;
    let (summary, files) = run(temp.path())?;
    assert_eq!(summary.progress.files, 1);
    assert_eq!(files[0].risk, Risk::Unknown);
    assert!(long.as_os_str().len() > 260);
    Ok(())
}

#[test]
fn sibling_database_stays_outside_authorized_scan_and_favorites_are_protected() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("nt_qq/nt_data");
    fs::create_dir_all(root.join("Pic"))?;
    fs::create_dir_all(root.join("Emoji/personal_emoji/Ori"))?;
    fs::create_dir_all(
        root.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?
            .join("nt_db"),
    )?;
    fs::write(root.join("Pic/chat.png"), b"\x89PNG\r\n\x1a\n")?;
    fs::write(
        root.join("Emoji/personal_emoji/Ori/收藏.png"),
        b"\x89PNG\r\n\x1a\n",
    )?;
    fs::write(
        root.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?
            .join("nt_db/messages.db"),
        b"private database",
    )?;
    let (summary, files) = run(&root)?;
    assert_eq!(summary.progress.files, 2);
    assert_eq!(summary.by_risk[&Risk::Protected].count, 1);
    assert!(files
        .iter()
        .all(|f| !f.relative_path.contains("messages.db")));
    Ok(())
}

#[test]
fn cancellation_flushes_bounded_partial_result() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    for i in 0..1000 {
        fs::write(temp.path().join(format!("file_{i}")), b"test")?;
    }
    let (_, dir) = open_authorized(temp.path())?;
    let cancel = AtomicBool::new(false);
    let mut persisted = 0;
    let result = qqnt_scanner::scan(
        &dir,
        &StructuralAdapter.detect(&dir),
        &cancel,
        |_| {},
        |batch| {
            persisted += batch.len();
            cancel.store(true, Ordering::Relaxed);
            Ok(())
        },
    )?;
    assert!(result.cancelled);
    assert_eq!(persisted, 256);
    assert_eq!(result.progress.files, 256);
    Ok(())
}

#[test]
fn hardlinks_are_readonly_and_logical_bytes_are_explicit() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("one"), b"12345")?;
    fs::hard_link(temp.path().join("one"), temp.path().join("two"))?;
    assert_eq!(run(temp.path())?.0.progress.bytes, 10);
    assert_eq!(fs::read(temp.path().join("one"))?, b"12345");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_do_not_escape_or_cycle() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(outside.path().join("private.jpg"), b"secret")?;
    std::os::unix::fs::symlink(outside.path(), temp.path().join("escape"))?;
    std::os::unix::fs::symlink(temp.path(), temp.path().join("cycle"))?;
    std::os::unix::fs::symlink(
        outside.path().join("private.jpg"),
        temp.path().join("alias"),
    )?;
    let (summary, _) = run(temp.path())?;
    assert_eq!(summary.progress.files, 0);
    assert_eq!(summary.progress.skipped_links, 3);
    assert!(open_authorized(&temp.path().join("escape")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn permission_errors_preserve_other_files() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let denied = temp.path().join("denied");
    fs::create_dir(&denied)?;
    fs::write(denied.join("hidden"), b"private")?;
    fs::write(temp.path().join("visible"), b"public")?;
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000))?;
    let result = run(temp.path());
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700))?;
    let summary = result?.0;
    assert_eq!(summary.progress.files, 1);
    assert_eq!(summary.progress.errors, 1);
    Ok(())
}

#[test]
fn rescan_detects_changed_and_removed_files() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("a"), b"old")?;
    assert_eq!(run(temp.path())?.0.progress.bytes, 3);
    fs::write(temp.path().join("a"), b"longer changed content")?;
    assert_eq!(run(temp.path())?.0.progress.bytes, 22);
    fs::remove_file(temp.path().join("a"))?;
    assert_eq!(run(temp.path())?.0.progress.files, 0);
    Ok(())
}

#[test]
#[ignore = "explicit stress test creates 100001 real files in a temporary directory"]
fn more_than_100k_files_reports_progress_and_bounded_batches() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    for d in 0..101 {
        let dir = temp.path().join(format!("d{d}"));
        fs::create_dir(&dir)?;
        let count = if d == 100 { 1 } else { 1000 };
        for f in 0..count {
            fs::write(dir.join(format!("{f}.dat")), b"payload")?;
        }
    }
    let (_, dir) = open_authorized(temp.path())?;
    let mut persisted = 0;
    let mut updates = 0;
    let summary = qqnt_scanner::scan(
        &dir,
        &StructuralAdapter.detect(&dir),
        &AtomicBool::new(false),
        |_| updates += 1,
        |batch| {
            assert!(batch.len() <= qqnt_scanner::BATCH_SIZE);
            persisted += batch.len();
            Ok(())
        },
    )?;
    assert_eq!(persisted, 100001);
    assert_eq!(summary.progress.bytes, 700007);
    assert!(updates > 1);
    eprintln!(
        "100001 files: {}ms, {} progress updates, batches <=256",
        summary.progress.elapsed_ms, updates
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn junction_is_skipped_without_admin() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(outside.path().join("secret"), b"private")?;
    let junction = temp.path().join("junction");
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .status()?;
    assert!(status.success());
    let result = run(temp.path());
    fs::remove_dir(&junction)?;
    let summary = result?.0;
    assert_eq!(summary.progress.files, 0);
    assert_eq!(summary.progress.skipped_links, 1);
    Ok(())
}

#[cfg(windows)]
#[test]
fn locked_file_does_not_abort_scan() -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("locked.jpg"), b"\xff\xd8\xff")?;
    fs::write(temp.path().join("readable"), b"test")?;
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(temp.path().join("locked.jpg"))?;
    let summary = run(temp.path())?.0;
    assert!(summary.progress.errors >= 1);
    assert!(summary.progress.files >= 1);
    Ok(())
}
