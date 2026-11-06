use qqnt_indexer::Index;
use qqnt_model::{Detection, FileQuery, IndexedFile, MediaKind, Risk, ScanSummary};

#[test]
fn sqlite_runtime_includes_wal_reset_fix() {
    assert!(
        rusqlite::version_number() >= 3_051_003,
        "SQLite must include the upstream concurrent WAL reset fix"
    );
}

fn detection() -> Detection {
    Detection {
        adapter: "test".into(),
        version: "1".into(),
        recognized: false,
        read_only: true,
        evidence: vec!["fixture".into()],
    }
}
fn file(path: &str, size: u64) -> IndexedFile {
    IndexedFile {
        relative_path: path.into(),
        directory: "Pic".into(),
        size,
        created: None,
        modified: Some(100),
        kind: MediaKind::Image,
        risk: Risk::Unknown,
        evidence: vec!["test evidence".into()],
        fingerprint: None,
        user_protected: false,
    }
}

#[test]
fn migration_reopen_dedup_queries_and_snapshots() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("index.db");
    let mut index = Index::open(&db)?;
    index.begin("first", "root", &detection())?;
    index.append(
        "first",
        &[
            file("Pic/中文.png", 10),
            file("Pic/a.jpg", 100),
            file("Pic/wild%_card.jpg", 60),
        ],
    )?;
    index.append("first", &[file("Pic/中文.png", 20)])?;
    let base = FileQuery {
        session_id: "first".into(),
        limit: 1,
        ..Default::default()
    };
    let first = index.files(&base)?;
    assert_eq!(first.total, 3);
    assert_eq!(first.files[0].size, 100);
    let second = index.files(&FileQuery {
        offset: 1,
        ..base.clone()
    })?;
    assert_eq!(second.files[0].size, 60);
    assert_eq!(
        index
            .files(&FileQuery {
                search: "中文".into(),
                ..base.clone()
            })?
            .files[0]
            .size,
        20
    );
    assert_eq!(
        index
            .files(&FileQuery {
                search: "%_".into(),
                ..base.clone()
            })?
            .total,
        1
    );
    assert_eq!(
        index
            .files(&FileQuery {
                search: "' OR 1=1 --".into(),
                ..base.clone()
            })?
            .total,
        0
    );
    assert_eq!(
        index
            .files(&FileQuery {
                min_size: Some(50),
                modified_from: Some(100),
                modified_to: Some(101),
                ..base.clone()
            })?
            .total,
        2
    );
    index.finish("first", &ScanSummary::default())?;
    index.begin("second", "root", &detection())?;
    index.append("second", &[file("Pic/new.png", 200)])?;
    index.recover_interrupted()?;
    drop(index);
    let index = Index::open(&db)?;
    assert_eq!(index.files(&base)?.total, 3);
    assert_eq!(index.sessions()?.len(), 2);
    assert_eq!(index.sessions()?[0].status, "interrupted");
    assert!(index.file("first", "../../private")?.is_none());
    Ok(())
}

#[test]
fn future_schema_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("future.db");
    let conn = rusqlite::Connection::open(&db)?;
    conn.execute_batch("PRAGMA user_version=999;")?;
    drop(conn);
    assert!(matches!(
        Index::open(&db),
        Err(qqnt_indexer::Error::FutureSchema)
    ));
    Ok(())
}

#[test]
fn upgrades_v1_and_recovers_interrupted_operations() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("v1.db");
    let conn = rusqlite::Connection::open(&db)?;
    conn.execute_batch(include_str!("../src/schema.sql"))?;
    drop(conn);
    let index = Index::open(&db)?;
    index.save_plan("plan", &serde_json::json!({"state":"running"}))?;
    index.save_operation(
        "op",
        "plan",
        "running",
        &serde_json::json!({"finished":null,"succeeded":1}),
    )?;
    index.recover_interrupted()?;
    let plan: serde_json::Value = index.plan("plan")?.ok_or("missing plan")?;
    assert_eq!(plan["state"], "interrupted");
    assert_eq!(index.operations()?[0]["status"], "interrupted");
    drop(index);
    let conn = rusqlite::Connection::open(db)?;
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?,
        4
    );
    let (status, payload): (String, String) = conn.query_row(
        "SELECT status,payload FROM cleanup_operations WHERE id='op'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(status, "interrupted");
    assert!(serde_json::from_str::<serde_json::Value>(&payload)?["finished"].is_i64());
    Ok(())
}

fn identified(path: &str, identity: &str, size: u64) -> IndexedFile {
    let mut entry = file(path, size);
    entry.risk = Risk::ReviewRequired;
    entry.fingerprint = Some(qqnt_model::Fingerprint {
        identity: identity.into(),
        size,
        modified_nanos: Some("100".into()),
        created_nanos: None,
        change_time: Some("1".into()),
        links: 1,
    });
    entry
}
fn summary(files: &[IndexedFile]) -> ScanSummary {
    let mut value = ScanSummary::default();
    for file in files {
        value.progress.files += 1;
        value.progress.bytes += file.size;
        value.by_kind.entry(file.kind).or_default().add(file.size);
        value.by_risk.entry(file.risk).or_default().add(file.size);
        value
            .by_directory
            .entry(file.directory.clone())
            .or_default()
            .add(file.size);
        value
            .by_month
            .entry("1970-01".into())
            .or_default()
            .add(file.size);
    }
    value
}

#[test]
fn known_removals_update_views_idempotently_without_changing_snapshots(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("index.db");
    let mut index = Index::open(&db)?;
    let files = vec![
        identified("Pic/a.png", "a", 10),
        identified("Pic/b.png", "b", 20),
    ];
    index.begin("scan", "root", &detection())?;
    index.append("scan", &files)?;
    index.finish("scan", &summary(&files))?;
    index.protect("root", &["Pic/b.png".into()], true)?;
    for _ in 0..2 {
        index.save_quarantine_effect(
            "q",
            "root",
            "deleted",
            &serde_json::json!({"state":"deleted"}),
            &files[0],
        )?;
    }
    let view = index.session("scan")?.ok_or("missing scan")?;
    let current = view.summary.ok_or("missing totals")?;
    assert_eq!((current.progress.files, current.progress.bytes), (1, 20));
    assert_eq!(current.by_kind[&MediaKind::Image].bytes, 20);
    assert_eq!(current.by_risk[&Risk::Protected].count, 1);
    assert_eq!(current.by_directory["Pic"].bytes, 20);
    assert_eq!(current.by_month["1970-01"].bytes, 20);
    assert!(index.file("scan", "Pic/a.png")?.is_none());
    assert_eq!(
        index
            .files(&FileQuery {
                session_id: "scan".into(),
                limit: 200,
                ..Default::default()
            })?
            .total,
        1
    );
    assert_eq!(index.sessions()?.len(), 1);
    let raw: String = rusqlite::Connection::open(&db)?.query_row(
        "SELECT summary FROM scan_sessions WHERE id='scan'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(serde_json::from_str::<ScanSummary>(&raw)?.progress.files, 2);
    index.begin("next", "root", &detection())?;
    let replacement = identified("Pic/a.png", "new identity", 30);
    index.append("next", std::slice::from_ref(&replacement))?;
    index.finish("next", &summary(&[replacement]))?;
    assert!(index.file("next", "Pic/a.png")?.is_some());
    Ok(())
}

#[test]
fn captured_selection_spans_pages_and_survives_removal_of_prior_batches(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut index = Index::open(&temp.path().join("index.db"))?;
    let files: Vec<_> = (0..6001)
        .map(|n| identified(&format!("Pic/{n:05}.png"), &n.to_string(), 1))
        .collect();
    index.begin("large", "root", &detection())?;
    index.append("large", &files)?;
    index.finish("large", &summary(&files))?;
    let query = FileQuery {
        session_id: "large".into(),
        kind: Some(MediaKind::Image),
        search: "Pic/".into(),
        offset: 5000,
        limit: 60,
        ..Default::default()
    };
    assert_eq!(
        index.capture_query(&query, &["Pic/00042.png".into()])?,
        6000
    );
    let mut after = String::new();
    let mut seen = std::collections::HashSet::new();
    let mut removals = 0;
    loop {
        let batch = index.selection_batch(&after, 64)?;
        if batch.is_empty() {
            break;
        }
        assert!(batch.len() <= 64);
        after = batch.last().ok_or("missing cursor")?.clone();
        for path in &batch {
            assert!(seen.insert(path.clone()));
        }
        let entry = index
            .file("large", &batch[0])?
            .ok_or("missing selected file")?;
        index.save_quarantine_effect(
            &format!("q-{removals}"),
            "root",
            "deleted",
            &serde_json::json!({}),
            &entry,
        )?;
        removals += 1;
    }
    assert_eq!(seen.len(), 6000);
    assert!(!seen.contains("Pic/00042.png"));
    assert_eq!(
        index
            .session("large")?
            .ok_or("missing session")?
            .summary
            .ok_or("missing summary")?
            .progress
            .files,
        6001 - removals
    );
    index.capture_query(
        &FileQuery {
            min_size: Some(2),
            ..query.clone()
        },
        &[],
    )?;
    assert!(index.selection_batch("", 200)?.is_empty());
    assert!(index
        .capture_paths("large", &["not-indexed".into()])
        .is_err());
    assert_eq!(index.capture_query(&query, &[])?, 6001 - removals);
    index.protect_captured("root", true)?;
    assert!(index
        .files(&FileQuery {
            offset: 0,
            limit: 200,
            ..query
        })?
        .files
        .iter()
        .all(|file| file.user_protected));
    Ok(())
}

#[test]
#[ignore = "explicit 100001-row bounded bulk-selection benchmark"]
fn hundred_thousand_indexed_paths_are_captured_without_page_limits(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut index = Index::open(&temp.path().join("index.db"))?;
    index.begin("stress", "root", &detection())?;
    for start in (0..100001).step_by(1000) {
        let rows: Vec<_> = (start..(start + 1000).min(100001))
            .map(|n| identified(&format!("Pic/{n:06}.png"), &n.to_string(), 1))
            .collect();
        index.append("stress", &rows)?;
    }
    let now = std::time::Instant::now();
    assert_eq!(
        index.capture_query(
            &FileQuery {
                session_id: "stress".into(),
                offset: 10000,
                limit: 60,
                ..Default::default()
            },
            &[]
        )?,
        100001
    );
    let mut cursor = String::new();
    let mut count = 0;
    loop {
        let batch = index.selection_batch(&cursor, 64)?;
        if batch.is_empty() {
            break;
        }
        assert!(batch.len() <= 64);
        count += batch.len();
        cursor = batch.last().ok_or("empty batch")?.clone();
    }
    assert_eq!(count, 100001);
    eprintln!(
        "captured and paged 100001 indexed paths in {:?}; <=64 paths per batch",
        now.elapsed()
    );
    Ok(())
}
