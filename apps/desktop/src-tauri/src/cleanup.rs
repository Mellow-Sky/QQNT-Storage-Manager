use crate::service::{Backend, Result};
use qqnt_cleaner::{CleanupMode, CleanupOperation, CleanupPlan, OperationFile, QuarantineItem};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequest {
    pub session_id: String,
    pub paths: Vec<String>,
    pub mode: CleanupMode,
    pub media_review: bool,
    pub rules: Option<qqnt_cleaner::CleanupRule>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantinePage {
    pub total: u64,
    pub items: Vec<QuarantineItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted: Vec<String>,
    pub retained: Vec<String>,
    pub failures: Vec<DeleteFailure>,
    pub bytes: u64,
    pub total: u64,
    pub processed: u64,
    pub deleted_count: u64,
    pub retained_count: u64,
    pub failed_count: u64,
    pub remaining: u64,
    pub cancelled: bool,
    pub stopped_reason: Option<String>,
    pub session: Option<qqnt_model::ScanSession>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFailure {
    pub path: String,
    pub reason: String,
}

enum DeleteScope<'a> {
    Paths(&'a [String]),
    Matching(&'a qqnt_model::FileQuery, &'a [String]),
}

impl Backend {
    pub fn import_quarantine(&self, grant_id: &str) -> Result<u64> {
        use std::io::Read;
        let _guard = self.begin_mutation("configuring")?;
        let (grant, root) = self.root_for_grant(grant_id)?;
        qqnt_adapters::reject_links(&root, Path::new(qqnt_cleaner::VAULT))
            .map_err(|_| "此目录没有可读的隔离区")?;
        let vault = root
            .open_dir(qqnt_cleaner::VAULT)
            .map_err(|_| "隔离区不可访问")?;
        qqnt_adapters::reject_links(&vault, Path::new("owner")).map_err(|_| "所有权标记不安全")?;
        let mut owner = String::new();
        vault
            .open("owner")
            .map_err(|_| "所有权标记不可读")?
            .take(128)
            .read_to_string(&mut owner)
            .map_err(|_| "所有权标记无效")?;
        if Uuid::parse_str(&owner).is_err() {
            return Err("隔离区所有权格式无效".into());
        }
        let index = self.index()?;
        let key = format!("vault:{}", blake3::hash(grant.path.as_bytes()).to_hex());
        if index
            .setting::<String>(&key)
            .map_err(|e| e.to_string())?
            .is_some_and(|existing| existing != owner)
        {
            return Err("隔离区所有权与应用索引冲突，拒绝导入".into());
        }
        let mut count = 0;
        for entry in vault.entries().map_err(|_| "隔离区列表不可读")? {
            let entry = entry.map_err(|_| "隔离区条目不可读")?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name
                .strip_suffix(".json")
                .is_some_and(|id| Uuid::parse_str(id).is_ok())
            {
                continue;
            }
            let meta = vault
                .symlink_metadata(name)
                .map_err(|_| "隔离清单元数据不可读")?;
            if !meta.is_file() || qqnt_adapters::is_link(&meta) || meta.len() > 128 * 1024 {
                continue;
            }
            let mut raw = Vec::new();
            vault
                .open(name)
                .map_err(|_| "隔离清单不可读")?
                .take(128 * 1024 + 1)
                .read_to_end(&mut raw)
                .map_err(|_| "隔离清单读取失败")?;
            if raw.len() > 128 * 1024 {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) else {
                continue;
            };
            if value["schema"] != 1 {
                continue;
            }
            let Ok(mut item) = serde_json::from_value::<QuarantineItem>(value["item"].clone())
            else {
                continue;
            };
            if !matches!(item.state.as_str(), "pending" | "quarantined")
                || item.stored_name != format!("{}.bin", item.id)
                || format!("{}.json", item.id) != name
                || !qqnt_adapters::safe_relative(Path::new(&item.relative_path))
                || qqnt_classifier::protected_path(Path::new(&item.relative_path))
            {
                continue;
            }
            if let Some(existing) = index
                .quarantine::<QuarantineItem>(&item.id)
                .map_err(|e| e.to_string())?
            {
                if existing.state == "deleted" || existing.state == "restored" {
                    continue;
                }
            }
            let source = Path::new(qqnt_cleaner::VAULT).join(&item.stored_name);
            if qqnt_adapters::reject_links(&root, &source).is_err() {
                continue;
            }
            let mut file = root
                .open(&source)
                .map_err(|_| "隔离文件无法读取")?
                .into_std();
            let fp = qqnt_fs::fingerprint(&file).map_err(|_| "隔离文件身份无法读取")?;
            if fp.identity != item.fingerprint.identity
                || fp.links != 1
                || fp.size != item.size
                || qqnt_fs::content_hash(&mut file).map_err(|_| "隔离文件摘要读取失败")?
                    != item.digest
            {
                continue;
            }
            item.root = grant.path.clone();
            item.fingerprint = fp;
            item.state = "quarantined".into();
            item.error = Some("已从隔离清单核对并恢复索引；恢复不会覆盖已有文件".into());
            index
                .save_quarantine(&item.id, &item.root, &item.state, &item)
                .map_err(|e| e.to_string())?;
            count += 1;
        }
        index.set_setting(&key, &owner).map_err(|e| e.to_string())?;
        Ok(count)
    }
    pub fn cleanup_enabled(&self) -> Result<bool> {
        self.index()?
            .setting::<bool>("cleanup_enabled")
            .map(|v| v.unwrap_or(false))
            .map_err(|e| e.to_string())
    }
    pub fn enable_cleanup(&self, enabled: bool) -> Result<()> {
        let _guard = self.begin_mutation("configuring")?;
        let index = self.index()?;
        // Turning cleanup off also revokes the persisted automatic-cache consent.
        if !enabled {
            let mut policy = self.policy_settings()?;
            policy.auto_cleanup = false;
            index
                .set_setting("lifecycle_policy", &policy)
                .map_err(|e| e.to_string())?;
        }
        index
            .set_setting("cleanup_enabled", &enabled)
            .map_err(|e| e.to_string())
    }
    pub fn plan_cleanup(&self, request: PlanRequest) -> Result<CleanupPlan> {
        if request.paths.is_empty() || request.paths.len() > 5000 {
            return Err("清理计划需要 1–5000 个具体文件".into());
        }
        let index = self.index()?;
        let session = index
            .session_metadata(&request.session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        if session.status != "completed" {
            return Err("只能从完整扫描结果生成清理计划".into());
        }
        let (grant, root) = self.authorized_root(&session.root)?;
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for path in request.paths {
            if !seen.insert(path.clone()) {
                continue;
            }
            files.push(
                index
                    .file(&session.id, &path)
                    .map_err(|e| e.to_string())?
                    .ok_or("计划包含不在索引中的文件")?,
            );
        }
        if let Some(rules) = &request.rules {
            rules.validate()?;
            if request.mode != CleanupMode::Advanced {
                return Err("自定义规则只允许用于高级模式".into());
            }
        }
        let mut plan = qqnt_cleaner::dry_run(
            &root,
            &session.id,
            &session.root,
            &grant.detection.version,
            grant.detection.recognized,
            request.mode,
            request.media_review,
            files,
        );
        if let Some(rules) = request.rules {
            for entry in &mut plan.entries {
                if entry.allowed && !rules.matches(&entry.file, chrono::Utc::now().timestamp()) {
                    entry.allowed = false;
                    entry.reason = "不满足高级模式自定义规则".into();
                }
            }
            plan.files = plan.entries.iter().filter(|e| e.allowed).count() as u64;
            plan.bytes = plan
                .entries
                .iter()
                .filter(|e| e.allowed)
                .map(|e| e.file.size)
                .sum();
            plan.rules = Some(rules);
        }
        index
            .save_plan(&plan.id, &plan)
            .map_err(|e| e.to_string())?;
        Ok(plan)
    }
    fn owner(&self, root: &str) -> Result<String> {
        let index = self.index()?;
        let key = format!("vault:{}", blake3::hash(root.as_bytes()).to_hex());
        if let Some(owner) = index.setting::<String>(&key).map_err(|e| e.to_string())? {
            return Ok(owner);
        }
        let owner = Uuid::new_v4().to_string();
        index.set_setting(&key, &owner).map_err(|e| e.to_string())?;
        Ok(owner)
    }
    pub fn execute_cleanup(&self, id: &str, confirmation: &str) -> Result<CleanupOperation> {
        self.execute_plan(id, confirmation, false, qqnt_fs::qq_is_running)
    }
    pub fn delete_files(&self, session_id: String, paths: Vec<String>) -> Result<DeleteResult> {
        self.delete_selection(session_id, paths, qqnt_fs::qq_is_running)
    }
    fn delete_selection(
        &self,
        session_id: String,
        paths: Vec<String>,
        process_check: fn() -> std::io::Result<bool>,
    ) -> Result<DeleteResult> {
        if paths.is_empty() || paths.iter().map(String::len).sum::<usize>() > 64 * 1024 * 1024 {
            return Err("请选择文件或缩小选择范围".into());
        }
        self.delete_scope(&session_id, DeleteScope::Paths(&paths), process_check)
    }
    pub fn delete_matching(
        &self,
        query: qqnt_model::FileQuery,
        excluded: Vec<String>,
    ) -> Result<DeleteResult> {
        self.delete_scope(
            &query.session_id,
            DeleteScope::Matching(&query, &excluded),
            qqnt_fs::qq_is_running,
        )
    }
    fn delete_scope(
        &self,
        session_id: &str,
        scope: DeleteScope<'_>,
        process_check: fn() -> std::io::Result<bool>,
    ) -> Result<DeleteResult> {
        use std::sync::atomic::Ordering;
        const BATCH: u32 = 64;
        const DETAILS: usize = 200;
        let (_guard, cancel) = self.begin_delete(session_id)?;
        let mut index = self.index()?;
        let session = index
            .session_metadata(session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        if session.status != "completed" {
            return Err("请先完成扫描".into());
        }
        self.authorized_root(&session.root)?;
        if process_check().map_err(|_| "无法检查 QQ 进程，请稍后重试")? {
            return Err("请先退出 QQ 后重试".into());
        }
        let total = match scope {
            DeleteScope::Paths(paths) => index.capture_paths(session_id, paths),
            DeleteScope::Matching(query, excluded) => index.capture_query(query, excluded),
        }
        .map_err(|e| e.to_string())?;
        let mut result = DeleteResult {
            deleted: vec![],
            retained: vec![],
            failures: vec![],
            bytes: 0,
            total,
            processed: 0,
            deleted_count: 0,
            retained_count: 0,
            failed_count: 0,
            remaining: total,
            cancelled: false,
            stopped_reason: None,
            session: None,
        };
        let publish = |result: &DeleteResult| {
            self.update_delete_progress(crate::service::DeleteProgress {
                total: result.total,
                processed: result.processed,
                deleted: result.deleted_count,
                failed: result.failed_count,
                bytes: result.bytes,
            })
        };
        publish(&result);
        let mut after = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                result.cancelled = true;
                break;
            }
            let paths = index
                .selection_batch(&after, BATCH)
                .map_err(|e| e.to_string())?;
            if paths.is_empty() {
                break;
            }
            if process_check().map_err(|_| "无法检查 QQ 进程，请稍后重试")? {
                result.stopped_reason = Some("QQ 已启动，删除已停止".into());
                break;
            }
            after = paths.last().cloned().unwrap_or_default();
            let plan = self.plan_cleanup(PlanRequest {
                session_id: session_id.into(),
                paths,
                mode: CleanupMode::Standard,
                media_review: true,
                rules: None,
            })?;
            for entry in plan.entries.iter().filter(|entry| !entry.allowed) {
                result.processed += 1;
                result.failed_count += 1;
                if result.failures.len() < DETAILS {
                    result.failures.push(DeleteFailure {
                        path: entry.file.relative_path.clone(),
                        reason: entry.reason.clone(),
                    });
                }
            }
            if plan.files > 0 {
                let operation = self.execute_plan_body(
                    &plan.id,
                    &format!("隔离 {} 个文件", plan.files),
                    true,
                    process_check,
                    Some(&cancel),
                )?;
                result.bytes += operation.logical_bytes;
                result.processed += operation.files.len() as u64;
                result.deleted_count += operation.succeeded;
                result.failed_count += operation.failed;
                for file in operation.files {
                    if file.state == "deleted" {
                        if result.deleted.len() < DETAILS {
                            result.deleted.push(file.relative_path);
                        }
                    } else {
                        if let Some(id) = &file.quarantine_id {
                            if index
                                .quarantine::<QuarantineItem>(id)
                                .map_err(|e| e.to_string())?
                                .is_some_and(|item| item.state == "quarantined")
                            {
                                result.retained_count += 1;
                                if result.retained.len() < DETAILS {
                                    result.retained.push(file.relative_path.clone());
                                }
                            }
                        }
                        if result.failures.len() < DETAILS {
                            result.failures.push(DeleteFailure {
                                path: file.relative_path,
                                reason: file.reason,
                            });
                        }
                    }
                }
            }
            result.remaining = total.saturating_sub(result.processed);
            publish(&result);
        }
        result.remaining = total.saturating_sub(result.processed);
        result.session = index.session(session_id).map_err(|e| e.to_string())?;
        publish(&result);
        Ok(result)
    }
    fn execute_plan(
        &self,
        id: &str,
        confirmation: &str,
        direct_delete: bool,
        process_check: fn() -> std::io::Result<bool>,
    ) -> Result<CleanupOperation> {
        let _guard = self.begin_mutation("cleaning")?;
        self.execute_plan_body(id, confirmation, direct_delete, process_check, None)
    }
    fn execute_plan_body(
        &self,
        id: &str,
        confirmation: &str,
        direct_delete: bool,
        process_check: fn() -> std::io::Result<bool>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<CleanupOperation> {
        #[cfg(not(windows))]
        let _ = process_check;
        if !direct_delete && !self.cleanup_enabled()? {
            return Err("清理默认关闭，请先在设置中明确启用".into());
        }
        let index = self.index()?;
        let mut plan: CleanupPlan = index
            .plan(id)
            .map_err(|e| e.to_string())?
            .ok_or("清理计划不存在")?;
        if confirmation != format!("隔离 {} 个文件", plan.files) {
            return Err("确认文字与计划不一致".into());
        }
        let now = chrono::Utc::now().timestamp();
        if plan.state != "draft" || plan.expires < now || plan.created > now + 60 {
            return Err("计划已执行、已过期或时钟异常，请重新模拟".into());
        }
        if plan.files == 0 {
            return Err("计划没有允许处理的文件".into());
        }
        let (grant, root) = self.authorized_root(&plan.root)?;
        if grant.detection.version != plan.adapter_version || !grant.detection.recognized {
            return Err("适配器或目录布局已改变，请重新扫描".into());
        }
        let owner = self.owner(&plan.root)?;
        plan.state = "running".into();
        index
            .save_plan(&plan.id, &plan)
            .map_err(|e| e.to_string())?;
        let mut operation = CleanupOperation {
            id: Uuid::new_v4().to_string(),
            plan_id: plan.id.clone(),
            action: if direct_delete {
                "delete"
            } else {
                "quarantine"
            }
            .into(),
            started: now,
            finished: None,
            files: vec![],
            succeeded: 0,
            failed: 0,
            logical_bytes: 0,
        };
        index
            .save_operation(&operation.id, &plan.id, "running", &operation)
            .map_err(|e| e.to_string())?;
        for entry in plan.entries.iter().filter(|e| e.allowed) {
            if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
                break;
            }
            let mut item = qqnt_cleaner::pending_item(&plan, entry)?;
            index
                .save_quarantine(&item.id, &item.root, &item.state, &item)
                .map_err(|e| e.to_string())?;
            let result = (|| -> Result<()> {
                if index
                    .is_protected(&plan.root, &entry.file.relative_path)
                    .map_err(|e| e.to_string())?
                {
                    return Err("用户已将此文件标为保护".into());
                }
                if !direct_delete && !self.cleanup_enabled()? {
                    return Err("清理已关闭".into());
                }
                #[cfg(windows)]
                {
                    qqnt_cleaner::mutation::quarantine(
                        &root,
                        Path::new(&plan.root),
                        &owner,
                        entry,
                        plan.mode,
                        plan.media_review,
                        grant.detection.recognized,
                        &mut item,
                        process_check,
                    )?;
                    if direct_delete {
                        qqnt_cleaner::mutation::purge(
                            &root,
                            Path::new(&plan.root),
                            &owner,
                            &mut item,
                            process_check,
                        )?;
                    }
                    return Ok(());
                }
                #[cfg(not(windows))]
                {
                    let _ = (&root, &owner);
                    Err("实际 QQ 文件清理仅支持 Windows".into())
                }
            })();
            match result {
                Ok(()) => {
                    operation.succeeded += 1;
                    operation.logical_bytes += item.size;
                    operation.files.push(OperationFile {
                        relative_path: item.relative_path.clone(),
                        state: item.state.clone(),
                        reason: if direct_delete {
                            "已删除"
                        } else {
                            "已隔离，同盘隔离尚未释放空间"
                        }
                        .into(),
                        quarantine_id: Some(item.id.clone()),
                    });
                }
                Err(reason) => {
                    operation.failed += 1;
                    item.error = Some(reason.clone());
                    self.reconcile_item(&mut item);
                    operation.files.push(OperationFile {
                        relative_path: item.relative_path.clone(),
                        state: "failed".into(),
                        reason,
                        quarantine_id: Some(item.id.clone()),
                    });
                }
            }
            if matches!(item.state.as_str(), "quarantined" | "deleted") {
                index
                    .save_quarantine_effect(&item.id, &item.root, &item.state, &item, &entry.file)
                    .map_err(|e| e.to_string())?;
            } else {
                index
                    .save_quarantine(&item.id, &item.root, &item.state, &item)
                    .map_err(|e| e.to_string())?;
            }
            index
                .save_operation(&operation.id, &plan.id, "running", &operation)
                .map_err(|e| e.to_string())?;
        }
        operation.finished = Some(chrono::Utc::now().timestamp());
        plan.state =
            if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
                "cancelled"
            } else if operation.failed == 0 {
                "completed"
            } else {
                "partial"
            }
            .into();
        index
            .save_operation(&operation.id, &plan.id, &plan.state, &operation)
            .map_err(|e| e.to_string())?;
        index
            .save_plan(&plan.id, &plan)
            .map_err(|e| e.to_string())?;
        Ok(operation)
    }
    fn reconcile_item(&self, item: &mut QuarantineItem) {
        if item.state != "pending" {
            return;
        }
        let Ok((_, root)) = self.authorized_root(&item.root) else {
            return;
        };
        if Uuid::parse_str(&item.id).is_err() || item.stored_name != format!("{}.bin", item.id) {
            item.state = "needs_review".into();
            return;
        }
        let owner = self.index().and_then(|index| {
            index
                .setting::<String>(&format!(
                    "vault:{}",
                    blake3::hash(item.root.as_bytes()).to_hex()
                ))
                .map_err(|e| e.to_string())
        });
        if !qqnt_adapters::safe_relative(Path::new(&item.relative_path))
            || qqnt_classifier::protected_path(Path::new(&item.relative_path))
            || !owner.is_ok_and(|owner| {
                owner
                    .is_some_and(|owner| qqnt_cleaner::validate_vault(&root, &owner, false).is_ok())
            })
        {
            item.state = "needs_review".into();
            return;
        }
        let path = Path::new(qqnt_cleaner::VAULT).join(&item.stored_name);
        if qqnt_adapters::reject_links(&root, &path).is_ok() {
            if let Ok(file) = root.open(&path) {
                let mut file = file.into_std();
                if let Ok(fp) = qqnt_fs::fingerprint(&file) {
                    if fp.identity == item.fingerprint.identity
                        && fp.links == 1
                        && fp.size == item.size
                        && qqnt_fs::content_hash(&mut file).is_ok_and(|h| h == item.digest)
                    {
                        item.fingerprint = fp;
                        item.state = "quarantined".into();
                        return;
                    }
                }
            }
        }
        item.state = if root.symlink_metadata(&path).is_ok() {
            "needs_review"
        } else {
            "failed"
        }
        .into();
    }
    pub fn quarantine_page(&self, offset: u32) -> Result<QuarantinePage> {
        let index = self.index()?;
        let (total, mut items) = index
            .quarantine_page::<QuarantineItem>(offset, 100)
            .map_err(|e| e.to_string())?;
        for item in &mut items {
            if item.state == "pending" {
                self.reconcile_item(item);
                index
                    .save_quarantine(&item.id, &item.root, &item.state, item)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(QuarantinePage { total, items })
    }
    pub fn quarantine_action(
        &self,
        ids: Vec<String>,
        action: &str,
        confirmation: &str,
    ) -> Result<CleanupOperation> {
        self.archive_action(ids, action, confirmation, false, qqnt_fs::qq_is_running)
    }
    pub fn restore_files(&self, ids: Vec<String>) -> Result<CleanupOperation> {
        let count = ids.iter().collect::<HashSet<_>>().len();
        self.archive_action(
            ids,
            "restore",
            &format!("恢复 {count} 个文件"),
            true,
            qqnt_fs::qq_is_running,
        )
    }
    fn archive_action(
        &self,
        ids: Vec<String>,
        action: &str,
        confirmation: &str,
        explicit_restore: bool,
        process_check: fn() -> std::io::Result<bool>,
    ) -> Result<CleanupOperation> {
        #[cfg(not(windows))]
        let _ = process_check;
        if ids.is_empty() || ids.len() > 1000 || !matches!(action, "restore" | "delete") {
            return Err("隔离区操作参数无效".into());
        }
        let ids: Vec<_> = ids
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let verb = if action == "restore" {
            "恢复"
        } else {
            "永久删除"
        };
        if confirmation != format!("{verb} {} 个文件", ids.len()) {
            return Err("确认文字不一致".into());
        }
        let _guard = self.begin_mutation(if action == "restore" {
            "restoring"
        } else {
            "deleting"
        })?;
        if !explicit_restore && !self.cleanup_enabled()? {
            return Err("请先启用清理与隔离区操作".into());
        }
        let index = self.index()?;
        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let mut operation = CleanupOperation {
            id: id.clone(),
            plan_id: id.clone(),
            action: action.into(),
            started: now,
            finished: None,
            files: vec![],
            succeeded: 0,
            failed: 0,
            logical_bytes: 0,
        };
        index
            .save_plan(&id, &serde_json::json!({"action":action,"created":now}))
            .map_err(|e| e.to_string())?;
        for id in ids {
            let mut item: QuarantineItem = index
                .quarantine(&id)
                .map_err(|e| e.to_string())?
                .ok_or("隔离记录不存在")?;
            let result = (|| -> Result<()> {
                let (grant, root) = self.authorized_root(&item.root)?;
                #[cfg(windows)]
                let item_root = std::path::PathBuf::from(&item.root);
                let owner = self.owner(&item.root)?;
                if action == "delete"
                    && (!grant.detection.recognized
                        || index
                            .is_protected(&item.root, &item.relative_path)
                            .map_err(|e| e.to_string())?)
                {
                    return Err("目录未知或原文件已受用户保护，拒绝永久删除".into());
                }
                #[cfg(windows)]
                return if action == "restore" {
                    qqnt_cleaner::mutation::restore(
                        &root,
                        &item_root,
                        &owner,
                        &mut item,
                        process_check,
                    )
                } else {
                    qqnt_cleaner::mutation::purge(
                        &root,
                        &item_root,
                        &owner,
                        &mut item,
                        process_check,
                    )
                };
                #[cfg(not(windows))]
                {
                    let _ = (&root, &owner);
                    Err("隔离区写操作仅支持 Windows".into())
                }
            })();
            match result {
                Ok(()) => {
                    operation.succeeded += 1;
                    operation.logical_bytes += item.size;
                    operation.files.push(OperationFile {
                        relative_path: item.relative_path.clone(),
                        state: item.state.clone(),
                        reason: if action == "restore" {
                            "已恢复，未覆盖已有文件"
                        } else {
                            "已永久删除隔离文件"
                        }
                        .into(),
                        quarantine_id: Some(item.id.clone()),
                    });
                }
                Err(reason) => {
                    operation.failed += 1;
                    item.error = Some(reason.clone());
                    operation.files.push(OperationFile {
                        relative_path: item.relative_path.clone(),
                        state: "failed".into(),
                        reason,
                        quarantine_id: Some(item.id.clone()),
                    });
                }
            }
            index
                .save_quarantine(&item.id, &item.root, &item.state, &item)
                .map_err(|e| e.to_string())?;
            index
                .save_operation(&operation.id, &operation.plan_id, "running", &operation)
                .map_err(|e| e.to_string())?;
        }
        operation.finished = Some(chrono::Utc::now().timestamp());
        index
            .save_operation(
                &operation.id,
                &operation.plan_id,
                if operation.failed == 0 {
                    "completed"
                } else {
                    "partial"
                },
                &operation,
            )
            .map_err(|e| e.to_string())?;
        Ok(operation)
    }
    pub fn cleanup_history(&self) -> Result<Vec<serde_json::Value>> {
        self.index()?.operations().map_err(|e| e.to_string())
    }
}

#[cfg(all(test, windows))]
mod direct_tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };
    fn wait(backend: &std::sync::Arc<Backend>) -> Result<()> {
        let start = Instant::now();
        while backend.status()?.state == "running" {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("scan timeout".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    #[test]
    fn selected_files_delete_directly_without_enabling_background_cleanup(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let source = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        for directory in ["Pic", "Emoji", "nt_db"] {
            fs::create_dir(source.path().join(directory))?;
        }
        for path in ["Pic/delete.png", "Pic/keep.png"] {
            fs::write(
                source.path().join(path),
                include_bytes!("../../../../tests/fixtures/media/image.png"),
            )?;
        }
        fs::write(
            source.path().join("nt_db/chat.db"),
            b"SQLite format 3\0protected",
        )?;
        fs::write(source.path().join("Pic/unknown.bin"), b"unknown")?;
        let backend = Backend::new(data.path())?;
        let grant = backend.authorize(source.path())?;
        let session = backend.start(&grant.id)?;
        wait(&backend)?;
        backend.protect(&session, vec!["Pic/keep.png".into()], true)?;
        assert!(!backend.cleanup_enabled()?);
        let result = backend.delete_selection(
            session.clone(),
            vec![
                "Pic/delete.png".into(),
                "Pic/keep.png".into(),
                "nt_db/chat.db".into(),
                "Pic/unknown.bin".into(),
            ],
            || Ok(false),
        )?;
        assert_eq!(result.deleted, vec!["Pic/delete.png"]);
        assert_eq!(result.bytes, 134);
        assert_eq!(result.failures.len(), 3);
        assert!(result.retained.is_empty());
        assert!(!source.path().join("Pic/delete.png").exists());
        for path in ["Pic/keep.png", "nt_db/chat.db", "Pic/unknown.bin"] {
            assert!(source.path().join(path).exists());
        }
        assert!(!backend.cleanup_enabled()?);
        assert!(backend
            .quarantine_page(0)?
            .items
            .iter()
            .all(|item| item.state == "deleted"));
        assert!(backend
            .delete_files("forged-session".into(), vec!["Pic/keep.png".into()])
            .is_err());
        fs::write(
            source.path().join("Pic/delete.png"),
            include_bytes!("../../../../tests/fixtures/media/image.png"),
        )?;
        let second = backend.start(&grant.id)?;
        wait(&backend)?;
        let plan = backend.plan_cleanup(PlanRequest {
            session_id: second,
            paths: vec!["Pic/delete.png".into()],
            mode: CleanupMode::Standard,
            media_review: true,
            rules: None,
        })?;
        let (_, root) = backend.authorized_root(&plan.root)?;
        let owner = backend.owner(&plan.root)?;
        let mut item = qqnt_cleaner::pending_item(&plan, &plan.entries[0])?;
        qqnt_cleaner::mutation::quarantine(
            &root,
            Path::new(&plan.root),
            &owner,
            &plan.entries[0],
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false),
        )?;
        backend
            .index()?
            .save_quarantine(&item.id, &item.root, &item.state, &item)?;
        let restored =
            backend.archive_action(vec![item.id], "restore", "恢复 1 个文件", true, || {
                Ok(false)
            })?;
        assert_eq!(restored.succeeded, 1);
        assert!(source.path().join("Pic/delete.png").exists());
        assert!(!backend.cleanup_enabled()?);

        Ok(())
    }
    #[test]
    fn bulk_query_deletes_multiple_batches_and_updates_totals_without_rescanning(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let source = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        for folder in ["Pic", "Emoji", "nt_db"] {
            fs::create_dir(source.path().join(folder))?;
        }
        for n in 0..130 {
            fs::write(
                source.path().join(format!("Pic/{n:05}.png")),
                include_bytes!("../../../../tests/fixtures/media/image.png"),
            )?;
        }
        fs::write(source.path().join("nt_db/chat.db"), b"protected database")?;
        fs::write(source.path().join("Pic/unknown.bin"), b"unknown")?;
        let backend = Backend::new(data.path())?;
        let grant = backend.authorize(source.path())?;
        let session = backend.start(&grant.id)?;
        wait(&backend)?;
        let query = qqnt_model::FileQuery {
            session_id: session.clone(),
            limit: 200,
            ..Default::default()
        };
        let result = backend.delete_scope(
            &session,
            DeleteScope::Matching(&query, &["Pic/00042.png".into()]),
            || Ok(false),
        )?;
        assert_eq!(
            (
                result.total,
                result.processed,
                result.deleted_count,
                result.failed_count,
                result.remaining
            ),
            (131, 131, 129, 2, 0)
        );
        assert_eq!(
            result
                .session
                .as_ref()
                .and_then(|s| s.summary.as_ref())
                .ok_or("missing summary")?
                .progress
                .files,
            3
        );
        assert_eq!(backend.sessions()?.len(), 1);
        assert_eq!(backend.files(query.clone())?.total, 3);
        assert!(source.path().join("Pic/00042.png").exists());
        let second =
            backend
                .delete_selection(session.clone(), vec!["Pic/00042.png".into()], || Ok(false))?;
        let totals = second
            .session
            .and_then(|s| s.summary)
            .ok_or("missing updated totals")?;
        assert_eq!((totals.progress.files, totals.progress.bytes), (2, 25));
        assert_eq!(backend.sessions()?.len(), 1);
        assert_eq!(backend.files(query)?.total, 2);
        assert!(!backend.cleanup_enabled()?);
        Ok(())
    }
    #[test]
    fn cancelling_a_bulk_job_preserves_unprocessed_files_and_current_counts(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let source = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        for folder in ["Pic", "Emoji"] {
            fs::create_dir(source.path().join(folder))?;
        }
        for n in 0..257 {
            fs::write(
                source.path().join(format!("Pic/{n:05}.png")),
                include_bytes!("../../../../tests/fixtures/media/image.png"),
            )?;
        }
        let backend = Backend::new(data.path())?;
        let grant = backend.authorize(source.path())?;
        let session = backend.start(&grant.id)?;
        wait(&backend)?;
        let watcher = std::sync::Arc::clone(&backend);
        let task = std::thread::spawn(move || -> Result<()> {
            let start = Instant::now();
            loop {
                let status = watcher.status()?;
                if status
                    .delete_progress
                    .as_ref()
                    .is_some_and(|p| p.processed >= 64)
                {
                    return watcher.cancel_delete();
                }
                if start.elapsed() > Duration::from_secs(20) {
                    return Err("cancellation watchdog timed out".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let query = qqnt_model::FileQuery {
            session_id: session.clone(),
            limit: 200,
            ..Default::default()
        };
        let result =
            backend.delete_scope(&session, DeleteScope::Matching(&query, &[]), || Ok(false))?;
        task.join().map_err(|_| "watchdog panicked")??;
        assert!(result.cancelled && result.deleted_count > 0 && result.deleted_count < 257);
        assert_eq!(result.processed + result.remaining, 257);
        assert_eq!(backend.files(query)?.total, 257 - result.deleted_count);
        assert_eq!(backend.sessions()?.len(), 1);
        Ok(())
    }
}
