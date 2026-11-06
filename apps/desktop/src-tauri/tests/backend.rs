use qqnt_desktop::service::Backend;
use qqnt_model::FileQuery;
use std::{
    fs,
    time::{Duration, Instant},
};

fn wait(backend: &std::sync::Arc<Backend>) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    loop {
        let status = backend.status()?;
        if status.state != "running" {
            assert_eq!(status.state, "completed");
            return Ok(());
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn media_protection_incremental_scan_and_owned_cache_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    for folder in ["Pic", "Emoji", "Video"] {
        fs::create_dir(source.path().join(folder))?;
    }
    fs::write(
        source.path().join("Pic/中文.png"),
        include_bytes!("../../../../tests/fixtures/media/image.png"),
    )?;
    fs::write(
        source.path().join("Video/video.mp4"),
        include_bytes!("../../../../tests/fixtures/media/video.mp4"),
    )?;
    let backend = Backend::new(data.path())?;
    let grant = backend.authorize(source.path())?;
    let first = backend.start(&grant.id)?;
    wait(&backend)?;
    let preview = backend.preview(&first, "Pic/中文.png", false)?;
    assert_eq!((preview.original_width, preview.original_height), (64, 32));
    assert!(qqnt_indexer::Index::open(&data.path().join("index.db"))?
        .cache_entries()?
        .is_empty());
    backend.protect(&first, vec!["Pic/中文.png".into()], true)?;
    let page = backend.files(FileQuery {
        session_id: first.clone(),
        risk: Some(qqnt_model::Risk::Protected),
        limit: 60,
        ..Default::default()
    })?;
    assert_eq!(page.total, 1);
    assert!(page.files[0].user_protected);
    backend.protect(&first, vec!["Pic/中文.png".into()], false)?;
    let video = backend.video_token(&first, "Video/video.mp4")?;
    let chunk = backend.media.video_chunk(&video, Some("bytes=0-31"))?;
    assert_eq!(chunk.bytes.len(), 32);
    assert_eq!(chunk.mime, "video/mp4");
    assert!(backend.media.video_chunk("forged-token", None).is_err());
    let second = backend.start(&grant.id)?;
    wait(&backend)?;
    assert_eq!(backend.status()?.progress.reused_files, 2);
    let mut policy = backend.policy_settings()?;
    policy.disk_cache_enabled = true;
    backend.save_policy(policy)?;
    backend.preview(&second, "Pic/中文.png", false)?;
    assert_eq!(
        qqnt_indexer::Index::open(&data.path().join("index.db"))?
            .cache_entries()?
            .len(),
        1
    );
    assert!(backend.clean_app_cache(true, "清理应用缩略图缓存").is_err());
    backend.enable_cleanup(true)?;
    let result = backend.clean_app_cache(true, "清理应用缩略图缓存")?;
    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed, 0);
    assert!(source.path().join("Pic/中文.png").exists());
    fs::write(source.path().join("Pic/中文.png"), b"changed")?;
    assert!(backend.preview(&second, "Pic/中文.png", false).is_err());
    Ok(())
}

#[test]
fn plans_recheck_protection_changes_and_are_single_use() -> Result<(), Box<dyn std::error::Error>> {
    use qqnt_cleaner::CleanupMode;
    use qqnt_desktop::cleanup::PlanRequest;
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    for folder in ["Pic", "Emoji", "nt_db"] {
        fs::create_dir(source.path().join(folder))?;
    }
    for file in ["Pic/a.png", "Pic/b.png"] {
        fs::write(
            source.path().join(file),
            include_bytes!("../../../../tests/fixtures/media/image.png"),
        )?;
    }
    fs::write(
        source.path().join("nt_db/chat.db"),
        b"SQLite format 3\0protected",
    )?;
    let backend = Backend::new(data.path())?;
    let grant = backend.authorize(source.path())?;
    let session = backend.start(&grant.id)?;
    wait(&backend)?;
    let plan = backend.plan_cleanup(PlanRequest {
        session_id: session.clone(),
        paths: vec![
            "Pic/a.png".into(),
            "Pic/b.png".into(),
            "nt_db/chat.db".into(),
        ],
        mode: CleanupMode::Advanced,
        media_review: true,
        rules: None,
    })?;
    assert_eq!(plan.files, 2);
    assert!(
        !plan
            .entries
            .iter()
            .find(|e| e.file.relative_path == "nt_db/chat.db")
            .ok_or("missing database")?
            .allowed
    );
    assert!(backend.execute_cleanup(&plan.id, "隔离 2 个文件").is_err());
    backend.enable_cleanup(true)?;
    assert!(backend.execute_cleanup(&plan.id, "wrong").is_err());
    backend.protect(&session, vec!["Pic/a.png".into()], true)?;
    fs::write(source.path().join("Pic/b.png"), b"changed since plan")?;
    let op = backend.execute_cleanup(&plan.id, "隔离 2 个文件")?;
    assert_eq!((op.succeeded, op.failed), (0, 2));
    assert!(op.files[0].reason.contains("保护"));
    assert!(backend.execute_cleanup(&plan.id, "隔离 2 个文件").is_err());
    assert!(source.path().join("Pic/a.png").exists());
    assert_eq!(
        fs::read(source.path().join("nt_db/chat.db"))?,
        b"SQLite format 3\0protected"
    );
    Ok(())
}

#[test]
fn concurrent_index_queries_tolerate_sqlite_sidecar_churn() -> Result<(), Box<dyn std::error::Error>>
{
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    fs::write(source.path().join("test"), b"mock")?;
    let backend = Backend::new(data.path())?;
    let grant = backend.authorize(source.path())?;
    let finished = Arc::new(AtomicBool::new(false));
    let reading_backend = Arc::clone(&backend);
    let reading_finished = Arc::clone(&finished);
    let reader = std::thread::spawn(move || -> Result<usize, String> {
        let mut count = 0;
        while !reading_finished.load(Ordering::Relaxed) {
            reading_backend.sessions()?;
            reading_backend.cleanup_enabled()?;
            count += 1;
            std::thread::yield_now();
        }
        Ok(count)
    });
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for _ in 0..30 {
            backend.start(&grant.id)?;
            wait(&backend)?;
        }
        Ok(())
    })();
    finished.store(true, Ordering::Relaxed);
    let count = reader.join().map_err(|_| "reader panicked")??;
    result?;
    assert!(count > 0);
    Ok(())
}

#[cfg(windows)]
#[test]
fn recovers_quarantine_manifest_after_losing_application_index(
) -> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    for folder in ["Pic", "Emoji"] {
        fs::create_dir(source.path().join(folder))?;
    }
    fs::write(
        source.path().join("Pic/a.png"),
        include_bytes!("../../../../tests/fixtures/media/image.png"),
    )?;
    let backend = Backend::new(data.path())?;
    let grant = backend.authorize(source.path())?;
    let session = backend.start(&grant.id)?;
    wait(&backend)?;
    let plan = backend.plan_cleanup(qqnt_desktop::cleanup::PlanRequest {
        session_id: session,
        paths: vec!["Pic/a.png".into()],
        mode: qqnt_cleaner::CleanupMode::Standard,
        media_review: true,
        rules: None,
    })?;
    let path = source.path().canonicalize()?;
    let (_, root) = qqnt_adapters::open_authorized(&path)?;
    let mut item = qqnt_cleaner::pending_item(&plan, &plan.entries[0])?;
    qqnt_cleaner::mutation::quarantine(
        &root,
        &path,
        &uuid::Uuid::new_v4().to_string(),
        &plan.entries[0],
        plan.mode,
        true,
        true,
        &mut item,
        || Ok(false),
    )?;
    drop(backend);
    // A completely separate database must reconstruct from durable manifests.
    let recovered_data = tempfile::tempdir()?;
    let recovered = Backend::new(recovered_data.path())?;
    let grant = recovered.authorize(source.path())?;
    assert_eq!(recovered.import_quarantine(&grant.id)?, 1);
    let page = recovered.quarantine_page(0)?;
    assert_eq!(page.items[0].state, "quarantined");
    assert_eq!(page.items[0].relative_path, "Pic/a.png");
    assert!(!source.path().join("Pic/a.png").exists());
    Ok(())
}

#[test]
fn authorized_scan_roundtrip_persistence_and_path_escape_rejection(
) -> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    for dir in ["Pic", "Video", "nt_db"] {
        fs::create_dir(source.path().join(dir))?;
    }
    fs::write(source.path().join("Pic/中文.jpg"), b"\xff\xd8\xff1234")?;
    fs::write(source.path().join("nt_db/chat.db"), b"protected database")?;
    let backend = Backend::new(data.path())?;
    assert!(backend.start("forged-grant").is_err());
    let grant = backend.choose(source.path())?.remove(0);
    assert!(grant.detection.recognized);
    let session_id = backend.start(&grant.id)?;
    let start = Instant::now();
    loop {
        let status = backend.status()?;
        if status.state != "running" {
            assert_eq!(status.state, "completed");
            assert_eq!(status.progress.files, 2);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let files = backend.files(FileQuery {
        session_id: session_id.clone(),
        limit: 60,
        ..Default::default()
    })?;
    assert_eq!(files.total, 2);
    assert!(backend.reveal_target(&session_id, "../../private").is_err());
    assert!(backend.reveal_target(&session_id, "Pic/中文.jpg")?.exists());
    fs::remove_file(source.path().join("Pic/中文.jpg"))?;
    assert!(backend.reveal_target(&session_id, "Pic/中文.jpg").is_err());
    drop(backend);
    let reopened = Backend::new(data.path())?;
    assert_eq!(reopened.sessions()?.len(), 1);
    assert_eq!(
        reopened
            .files(FileQuery {
                session_id,
                limit: 60,
                ..Default::default()
            })?
            .total,
        2
    );
    Ok(())
}

#[test]
#[ignore = "explicit full pipeline stress test with 100001 real temporary files"]
fn hundred_thousand_files_index_while_status_remains_available(
) -> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    for d in 0..101 {
        let dir = source.path().join(format!("directory_{d}"));
        fs::create_dir(&dir)?;
        for f in 0..if d == 100 { 1 } else { 1000 } {
            fs::write(dir.join(format!("{f}.dat")), b"payload")?;
        }
    }
    let backend = Backend::new(data.path())?;
    let grant = backend.choose(source.path())?.remove(0);
    let session_id = backend.start(&grant.id)?;
    let start = Instant::now();
    let mut observations = 0;
    let mut observed_partial = false;
    loop {
        let status = backend.status()?;
        observations += 1;
        if status.state != "running" {
            assert_eq!(status.state, "completed");
            assert_eq!(status.progress.files, 100001);
            break;
        }
        observed_partial |= status.progress.files > 0;
        assert!(start.elapsed() < Duration::from_secs(180));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(observed_partial);
    let page = backend.files(FileQuery {
        session_id,
        limit: 60,
        ..Default::default()
    })?;
    assert_eq!(page.total, 100001);
    assert_eq!(page.files.len(), 60);
    eprintln!(
        "100001-file pipeline: {:?}, {} status observations; 60-row page",
        start.elapsed(),
        observations
    );
    Ok(())
}

#[test]
fn application_index_has_one_process_owner() -> Result<(), Box<dyn std::error::Error>> {
    let data = tempfile::tempdir()?;
    let first = Backend::new(data.path())?;
    assert!(Backend::new(data.path()).is_err());
    drop(first);
    let reopened = Backend::new(data.path())?;
    assert!(reopened.sessions()?.is_empty());
    Ok(())
}

#[test]
fn cannot_authorize_a_parent_of_the_application_index() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let backend = Backend::new(&root.path().join("app-data"))?;
    assert!(backend.authorize(root.path()).is_err());
    fs::create_dir(root.path().join("QQ-data"))?;
    assert!(backend.authorize(&root.path().join("QQ-data")).is_ok());
    Ok(())
}
