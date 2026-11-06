use crate::service::{Backend, Result};
use qqnt_cleaner::{CleanupOperation, OperationFile};
use qqnt_model::{FileQuery, FileSort, Risk};
use qqnt_policy::{PolicyReport, PolicySettings};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

impl Backend {
    pub fn policy_settings(&self) -> Result<PolicySettings> {
        self.index()?
            .setting("lifecycle_policy")
            .map(|s| s.unwrap_or_default())
            .map_err(|e| e.to_string())
    }
    pub fn save_policy(&self, settings: PolicySettings) -> Result<()> {
        let _guard = self.begin_mutation("configuring")?;
        settings.validate()?;
        if settings.auto_cleanup && !self.cleanup_enabled()? {
            return Err("自动回收前请先明确启用清理".into());
        }
        let index = self.index()?;
        self.media.clear_memory();
        index
            .set_setting("media_cache_enabled", &settings.disk_cache_enabled)
            .map_err(|e| e.to_string())?;
        index
            .set_setting("lifecycle_policy", &settings)
            .map_err(|e| e.to_string())
    }
    pub fn policy_report(&self, session_id: &str) -> Result<PolicyReport> {
        let settings = self.policy_settings()?;
        let index = self.index()?;
        let session = index
            .session(session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        let (_, root) = self.authorized_root(&session.root)?;
        let _ = root;
        let scope_bytes = session.summary.as_ref().map_or(0, |s| s.progress.bytes);
        let scope_over_limit = settings
            .scope_limit_bytes
            .is_some_and(|limit| scope_bytes > limit);
        let mut needed = settings
            .scope_limit_bytes
            .map_or(0, |limit| scope_bytes.saturating_sub(limit));
        let mut count = 0;
        let mut review_bytes = 0;
        let mut paths = Vec::new();
        let mut offset = 0;
        loop {
            let page = index
                .files(&FileQuery {
                    session_id: session_id.into(),
                    risk: Some(Risk::ReviewRequired),
                    sort: FileSort::ModifiedAsc,
                    offset,
                    limit: 200,
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?;
            for file in page.files {
                if qqnt_policy::review_candidate(&file, &settings, chrono::Utc::now().timestamp())
                    || needed > 0
                {
                    count += 1;
                    review_bytes += file.size;
                    needed = needed.saturating_sub(file.size);
                    if paths.len() < 5000 {
                        paths.push(file.relative_path);
                    }
                }
            }
            offset += 200;
            if offset as u64 >= page.total {
                break;
            }
        }
        let cache = index.cache_entries().map_err(|e| e.to_string())?;
        let cache_bytes = cache.iter().map(|c| c.size).sum();
        let reclaimable =
            qqnt_policy::cache_evictions(&cache, &settings, chrono::Utc::now().timestamp())
                .iter()
                .map(|c| c.size)
                .sum();
        let available = fs2::available_space(&session.root).ok();
        Ok(PolicyReport{review_count:count,review_bytes,review_paths:paths,scope_bytes,scope_over_limit,available_bytes:available,low_disk:available.is_some_and(|b|b<settings.low_disk_bytes),cache_count:cache.len() as u64,cache_bytes,cache_reclaimable_bytes:reclaimable,warning:"QQ 媒体过期或超出容量仅进入人工审核；自动清理只处理本应用生成且可重新生成的缩略图缓存。".into()})
    }
    pub fn clean_app_cache(&self, all: bool, confirmation: &str) -> Result<CleanupOperation> {
        let _guard = self.begin_mutation("cleaning")?;
        if !self.cleanup_enabled()? {
            return Err("请先启用清理".into());
        }
        if confirmation != "清理应用缩略图缓存" {
            return Err("确认文字不一致".into());
        }
        let settings = self.policy_settings()?;
        let index = self.index()?;
        let cache = index.cache_entries().map_err(|e| e.to_string())?;
        let selected = if all {
            cache
        } else {
            qqnt_policy::cache_evictions(&cache, &settings, chrono::Utc::now().timestamp())
        };
        let id = Uuid::new_v4().to_string();
        let mut op = CleanupOperation {
            id: id.clone(),
            plan_id: id.clone(),
            action: "app_cache".into(),
            started: chrono::Utc::now().timestamp(),
            finished: None,
            files: vec![],
            succeeded: 0,
            failed: 0,
            logical_bytes: 0,
        };
        index
            .save_plan(
                &id,
                &serde_json::json!({"action":"app_cache","created":op.started}),
            )
            .map_err(|e| e.to_string())?;
        index
            .save_operation(&id, &id, "running", &op)
            .map_err(|e| e.to_string())?;
        for entry in selected {
            let result = (|| -> Result<()> {
                if !crate::media::valid_filename(&entry.filename) {
                    return Err("不是本应用的缓存文件名".into());
                }
                let path = std::path::Path::new(&entry.filename);
                match self.media.disk().symlink_metadata(path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        index
                            .remove_cache_entry(&entry.key)
                            .map_err(|e| e.to_string())?;
                        return Ok(());
                    }
                    Err(_) => return Err("缓存元数据不可读".into()),
                    Ok(m)
                        if !m.is_file()
                            || qqnt_adapters::is_link(&m)
                            || m.len() != entry.size
                            || m.len() > 4 * 1024 * 1024 =>
                    {
                        return Err("缓存身份/大小或链接检查失败".into())
                    }
                    Ok(_) => {}
                }
                let expected = entry
                    .fingerprint
                    .as_ref()
                    .ok_or("缓存缺少创建时身份，拒绝自动删除")?;
                #[cfg(windows)]
                let mut locked =
                    qqnt_fs::windows::lock_file(self.media.disk(), self.media.disk_path(), path)
                        .map_err(|e| e.to_string())?;
                #[cfg(windows)]
                let opened = &mut locked.file;
                #[cfg(not(windows))]
                let mut file = self
                    .media
                    .disk()
                    .open(path)
                    .map_err(|_| "缓存无法读取")?
                    .into_std();
                #[cfg(not(windows))]
                let opened = &mut file;
                if qqnt_fs::fingerprint(opened).map_err(|_| "缓存身份读取失败")? != *expected
                {
                    return Err("缓存文件已被替换或修改".into());
                }
                if qqnt_fs::content_hash(opened).map_err(|_| "缓存摘要读取失败")? != entry.digest
                {
                    return Err("缓存摘要不匹配".into());
                }
                // This capability contains only application-owned, regenerable derivatives.
                #[cfg(windows)]
                locked.delete().map_err(|_| "缓存删除失败")?;
                #[cfg(not(windows))]
                self.media
                    .disk()
                    .remove_file(path)
                    .map_err(|_| "缓存删除失败")?;
                index
                    .remove_cache_entry(&entry.key)
                    .map_err(|e| e.to_string())?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    op.succeeded += 1;
                    op.logical_bytes += entry.size;
                }
                Err(reason) => {
                    op.failed += 1;
                    op.files.push(OperationFile {
                        relative_path: entry.filename,
                        state: "failed".into(),
                        reason,
                        quarantine_id: None,
                    });
                }
            }
            index
                .save_operation(&id, &id, "running", &op)
                .map_err(|e| e.to_string())?;
        }
        op.finished = Some(chrono::Utc::now().timestamp());
        index
            .save_operation(
                &id,
                &id,
                if op.failed == 0 {
                    "completed"
                } else {
                    "partial"
                },
                &op,
            )
            .map_err(|e| e.to_string())?;
        Ok(op)
    }
    pub fn launch_scheduler(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let Some(backend) = weak.upgrade() else {
                break;
            };
            let Ok(settings) = backend.policy_settings() else {
                continue;
            };
            if settings.auto_cleanup && backend.cleanup_enabled().unwrap_or(false) {
                if let Ok(index) = backend.index() {
                    if let Ok(entries) = index.cache_entries() {
                        if !qqnt_policy::cache_evictions(
                            &entries,
                            &settings,
                            chrono::Utc::now().timestamp(),
                        )
                        .is_empty()
                        {
                            let _ = backend.clean_app_cache(false, "清理应用缩略图缓存");
                        }
                    }
                }
            }
            if settings.auto_scan {
                let _ = backend.auto_scan_due(settings.scan_interval_minutes);
            }
        });
    }
}
