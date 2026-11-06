use cap_std::fs::Dir;
use qqnt_adapters::{open_authorized, reject_links, QQNTAdapter, StructuralAdapter};
use qqnt_indexer::Index;
use qqnt_model::{Detection, FilePage, FileQuery, ScanProgress, ScanSession, ScanSummary};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub id: String,
    pub path: String,
    pub detection: Detection,
}
struct Authorized {
    grant: Grant,
    dir: Arc<Dir>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProgress {
    pub total: u64,
    pub processed: u64,
    pub deleted: u64,
    pub failed: u64,
    pub bytes: u64,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub session_id: Option<String>,
    pub state: String,
    pub progress: ScanProgress,
    pub summary: Option<ScanSummary>,
    pub error: Option<String>,
    pub delete_progress: Option<DeleteProgress>,
}
struct Activity {
    status: Status,
    cancel: Arc<AtomicBool>,
}
pub struct Backend {
    db: PathBuf,
    // Keep WAL/SHM alive between short command connections. Mutex makes the
    // otherwise unused Connection safely owned by this shared backend.
    _index_lease: Mutex<Index>,
    grants: Mutex<HashMap<String, Authorized>>,
    activity: Mutex<Activity>,
    pub media: crate::media::MediaService,
    // Drop last, after SQLite and media handles have closed.
    _instance_lock: std::fs::File,
}

impl Backend {
    pub fn new(data_dir: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir).map_err(|_| "无法创建应用数据目录")?;
        let (canonical_data, data_scope) =
            open_authorized(data_dir).map_err(|_| "应用数据目录包含链接或重解析点")?;
        let instance_lock = lock_instance(&data_scope)?;
        validate_index_files(&data_scope)?;
        let db = canonical_data.join("index.db");
        let index_lease = Index::open(&db).map_err(|e| e.to_string())?;
        index_lease
            .recover_interrupted()
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            db,
            _instance_lock: instance_lock,
            _index_lease: Mutex::new(index_lease),
            grants: Mutex::new(HashMap::new()),
            activity: Mutex::new(Activity {
                status: Status {
                    state: "idle".into(),
                    ..Status::default()
                },
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            media: crate::media::MediaService::new(data_dir)?,
        }))
    }
    pub(crate) fn index(&self) -> Result<Index> {
        let parent = self.db.parent().ok_or("索引目录无效")?;
        let (_, scope) = open_authorized(parent).map_err(|_| "索引目录已变化或包含链接")?;
        validate_index_files(&scope)?;
        Index::open(&self.db).map_err(|e| e.to_string())
    }
    pub fn authorize(&self, path: &Path) -> Result<Grant> {
        let (canonical, dir) =
            open_authorized(path).map_err(|_| "无法授权目录：不存在、无权限或包含链接/重解析点")?;
        if self.db.starts_with(&canonical) {
            return Err("请选择更具体的数据目录，扫描范围不能包含应用自身索引".into());
        }
        let grant = Grant {
            id: Uuid::new_v4().to_string(),
            path: canonical.to_str().ok_or("路径编码不受支持")?.into(),
            detection: StructuralAdapter.detect(&dir),
        };
        let mut grants = self.grants.lock().map_err(|_| "授权状态不可用")?;
        // Reusing a retained handle also prevents unbounded grants on repeated discovery.
        if let Some(existing) = grants.values().find(|g| g.grant.path == grant.path) {
            return Ok(existing.grant.clone());
        }
        grants.insert(
            grant.id.clone(),
            Authorized {
                grant: grant.clone(),
                dir: Arc::new(dir),
            },
        );
        Ok(grant)
    }
    pub fn choose(&self, path: &Path) -> Result<Vec<Grant>> {
        let candidates = qqnt_discovery::candidates(path).unwrap_or_default();
        if candidates.is_empty() {
            return self.authorize(path).map(|g| vec![g]);
        }
        candidates.iter().map(|p| self.authorize(p)).collect()
    }
    pub fn discover(&self) -> Vec<Grant> {
        qqnt_discovery::default_containers()
            .iter()
            .flat_map(|base| qqnt_discovery::candidates(base).unwrap_or_default())
            .filter_map(|p| self.authorize(&p).ok())
            .collect()
    }
    pub fn status(&self) -> Result<Status> {
        Ok(self
            .activity
            .lock()
            .map_err(|_| "任务状态不可用")?
            .status
            .clone())
    }
    pub fn cancel(&self, session_id: &str) -> Result<()> {
        let activity = self.activity.lock().map_err(|_| "任务状态不可用")?;
        if activity.status.session_id.as_deref() != Some(session_id)
            || activity.status.state != "running"
        {
            return Err("扫描任务已经结束".into());
        }
        activity.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn start(self: &Arc<Self>, grant_id: &str) -> Result<String> {
        let (mut grant, dir) = {
            let grants = self.grants.lock().map_err(|_| "授权状态不可用")?;
            let authorized = grants.get(grant_id).ok_or("目录未授权，请先选择目录")?;
            (authorized.grant.clone(), Arc::clone(&authorized.dir))
        };
        grant.detection = StructuralAdapter.detect(&dir);
        let mut activity = self.activity.lock().map_err(|_| "任务状态不可用")?;
        if [
            "running",
            "cleaning",
            "restoring",
            "deleting",
            "configuring",
        ]
        .contains(&activity.status.state.as_str())
        {
            return Err("已有扫描任务，请等待或取消".into());
        }
        let id = Uuid::new_v4().to_string();
        self.index()?
            .begin(&id, &grant.path, &grant.detection)
            .map_err(|e| e.to_string())?;
        let cancel = Arc::new(AtomicBool::new(false));
        activity.status = Status {
            session_id: Some(id.clone()),
            state: "running".into(),
            ..Status::default()
        };
        activity.cancel = Arc::clone(&cancel);
        let backend = Arc::clone(self);
        let job = id.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<ScanSummary> {
                let mut index = backend.index()?;
                let cache_index = backend.index()?;
                let summary = qqnt_scanner::scan_incremental(
                    &dir,
                    &grant.detection,
                    &cancel,
                    |path, fp| {
                        cache_index
                            .cached(&grant.path, path, &grant.detection, fp)
                            .map_err(std::io::Error::other)
                    },
                    |p| {
                        if let Ok(mut a) = backend.activity.lock() {
                            a.status.progress = p.clone();
                        }
                    },
                    |batch| index.append(&job, batch).map_err(std::io::Error::other),
                )
                .map_err(|_| "扫描或索引写入失败，部分结果已保留".to_string())?;
                index.finish(&job, &summary).map_err(|e| e.to_string())?;
                backend.publish_summary(&summary, &grant.detection);
                Ok(summary)
            })();
            if let Ok(mut a) = backend.activity.lock() {
                match result {
                    Ok(summary) => {
                        a.status.state = if summary.cancelled {
                            "cancelled"
                        } else {
                            "completed"
                        }
                        .into();
                        a.status.progress = summary.progress.clone();
                        a.status.summary = Some(summary);
                    }
                    Err(error) => {
                        if let Ok(i) = backend.index() {
                            let _ = i.fail(&job);
                        }
                        tracing::error!("scan worker failed");
                        a.status.state = "failed".into();
                        a.status.error = Some(error);
                    }
                }
            }
        });
        Ok(id)
    }
    pub fn sessions(&self) -> Result<Vec<ScanSession>> {
        self.index()?.sessions().map_err(|e| e.to_string())
    }
    pub fn files(&self, query: FileQuery) -> Result<FilePage> {
        self.index()?.files(&query).map_err(|e| e.to_string())
    }
    pub fn indexed_media(
        &self,
        session_id: &str,
        path: &str,
    ) -> Result<(Arc<Dir>, qqnt_model::IndexedFile, String)> {
        let index = self.index()?;
        let session = index
            .session_metadata(session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        let file = index
            .file(session_id, path)
            .map_err(|e| e.to_string())?
            .ok_or("文件不在索引中")?;
        let grants = self.grants.lock().map_err(|_| "授权状态不可用")?;
        let root = grants
            .values()
            .find(|a| a.grant.path == session.root)
            .ok_or("请先重新选择对应目录，授权媒体读取")?;
        Ok((Arc::clone(&root.dir), file, session.root))
    }
    pub fn preview(
        &self,
        session: &str,
        path: &str,
        full: bool,
    ) -> Result<qqnt_model::MediaPreview> {
        let (root, file, root_path) = self.indexed_media(session, path)?;
        self.media
            .preview(&self.index()?, root, &root_path, &file, full)
    }
    pub fn video_token(&self, session: &str, path: &str) -> Result<String> {
        let (root, file, _) = self.indexed_media(session, path)?;
        self.media.token(root, file)
    }
    pub fn protect(&self, session: &str, paths: Vec<String>, value: bool) -> Result<()> {
        let _guard = self.begin_mutation("configuring")?;
        let mut index = self.index()?;
        let data = index
            .session_metadata(session)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        self.authorized_root(&data.root)?;
        if paths.is_empty() || paths.iter().map(String::len).sum::<usize>() > 64 * 1024 * 1024 {
            return Err("请选择文件或缩小选择范围".into());
        }
        index
            .capture_paths(session, &paths)
            .map_err(|e| e.to_string())?;
        index
            .protect_captured(&data.root, value)
            .map_err(|e| e.to_string())
    }
    pub fn protect_matching(
        &self,
        query: FileQuery,
        excluded: Vec<String>,
        value: bool,
    ) -> Result<()> {
        let _guard = self.begin_mutation("configuring")?;
        let mut index = self.index()?;
        let data = index
            .session_metadata(&query.session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        self.authorized_root(&data.root)?;
        index
            .capture_query(&query, &excluded)
            .map_err(|e| e.to_string())?;
        index
            .protect_captured(&data.root, value)
            .map_err(|e| e.to_string())
    }
    pub fn session(&self, id: &str) -> Result<ScanSession> {
        self.index()?
            .session(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "扫描会话不存在".into())
    }
    pub(crate) fn begin_delete(
        &self,
        session: &str,
    ) -> Result<(MutationGuard<'_>, Arc<AtomicBool>)> {
        let guard = self.begin_mutation("deleting")?;
        let mut activity = self.activity.lock().map_err(|_| "任务状态不可用")?;
        activity.cancel = Arc::new(AtomicBool::new(false));
        activity.status.session_id = Some(session.into());
        activity.status.delete_progress = Some(DeleteProgress::default());
        Ok((guard, Arc::clone(&activity.cancel)))
    }
    pub(crate) fn update_delete_progress(&self, progress: DeleteProgress) {
        if let Ok(mut activity) = self.activity.lock() {
            activity.status.delete_progress = Some(progress);
        }
    }
    pub fn cancel_delete(&self) -> Result<()> {
        let activity = self.activity.lock().map_err(|_| "任务状态不可用")?;
        if activity.status.state == "deleting" {
            activity.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
    pub(crate) fn authorized_root(&self, path: &str) -> Result<(Grant, Arc<Dir>)> {
        let grants = self.grants.lock().map_err(|_| "授权状态不可用")?;
        let a = grants
            .values()
            .find(|a| a.grant.path == path)
            .ok_or("请重新选择该数据目录以授权操作")?;
        let mut grant = a.grant.clone();
        grant.detection = StructuralAdapter.detect(&a.dir);
        Ok((grant, Arc::clone(&a.dir)))
    }
    pub(crate) fn begin_mutation(&self, state: &str) -> Result<MutationGuard<'_>> {
        let mut a = self.activity.lock().map_err(|_| "任务状态不可用")?;
        if [
            "running",
            "cleaning",
            "restoring",
            "deleting",
            "configuring",
        ]
        .contains(&a.status.state.as_str())
        {
            return Err("扫描或清理任务正在执行，请先结束当前任务".into());
        }
        a.status = Status {
            state: state.into(),
            ..Status::default()
        };
        Ok(MutationGuard { backend: self })
    }
    pub(crate) fn auto_scan_due(self: &Arc<Self>, minutes: u32) -> Result<()> {
        let grants: Vec<_> = self
            .grants
            .lock()
            .map_err(|_| "授权状态不可用")?
            .values()
            .map(|a| a.grant.clone())
            .collect();
        let index = self.index()?;
        for grant in grants {
            let last = index
                .latest_scan_time(&grant.path)
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if chrono::Utc::now().timestamp() - last >= minutes as i64 * 60 {
                self.start(&grant.id)?;
                break;
            }
        }
        Ok(())
    }
    pub fn disk_status(&self, grant_id: &str) -> Result<serde_json::Value> {
        let path = self
            .grants
            .lock()
            .map_err(|_| "授权状态不可用")?
            .get(grant_id)
            .ok_or("目录未授权")?
            .grant
            .path
            .clone();
        let threshold = self.policy_settings()?.low_disk_bytes;
        let available = fs2::available_space(path).map_err(|_| "磁盘剩余空间无法读取")?;
        Ok(
            serde_json::json!({"availableBytes":available,"thresholdBytes":threshold,"lowDisk":available<threshold}),
        )
    }
    pub(crate) fn root_for_grant(&self, id: &str) -> Result<(Grant, Arc<Dir>)> {
        let path = self
            .grants
            .lock()
            .map_err(|_| "授权状态不可用")?
            .get(id)
            .ok_or("目录未授权")?
            .grant
            .path
            .clone();
        self.authorized_root(&path)
    }
    pub fn reveal_target(&self, session_id: &str, relative: &str) -> Result<PathBuf> {
        let index = self.index()?;
        let session = index
            .session_metadata(session_id)
            .map_err(|e| e.to_string())?
            .ok_or("扫描会话不存在")?;
        if index
            .file(session_id, relative)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Err("文件不在扫描索引中".into());
        }
        let grants = self.grants.lock().map_err(|_| "授权状态不可用")?;
        let authorized = grants
            .values()
            .find(|a| a.grant.path == session.root)
            .ok_or("请重新选择该数据目录以授权定位文件")?;
        let path = Path::new(relative);
        reject_links(&authorized.dir, path).map_err(|_| "文件已移动、不可访问或路径包含链接")?;
        authorized
            .dir
            .open(path)
            .map_err(|_| "文件不存在或无法读取")?;
        let root = Path::new(&session.root);
        let target = root.join(path).canonicalize().map_err(|_| "文件已不存在")?;
        if !target.starts_with(root) {
            return Err("文件已离开授权范围".into());
        }
        Ok(target)
    }
}

fn lock_instance(root: &Dir) -> Result<std::fs::File> {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    let file = match root.open_with("manager.lock", &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            reject_links(root, Path::new("manager.lock")).map_err(|_| "应用实例锁路径不安全")?;
            options.create_new(false);
            root.open_with("manager.lock", &options)
                .map_err(|_| "应用实例锁不可访问")?
        }
        Err(_) => return Err("无法创建应用实例锁".into()),
    }
    .into_std();
    if qqnt_fs::fingerprint(&file)
        .map_err(|_| "应用实例锁身份无效")?
        .links
        != 1
    {
        return Err("应用实例锁存在硬链接或状态已变化".into());
    }
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "同一数据目录已有管理器实例在运行")?;
    Ok(file)
}

fn validate_index_files(root: &Dir) -> Result<()> {
    for path in [
        "index.db",
        "index.db-wal",
        "index.db-shm",
        "index.db-journal",
    ] {
        match root.symlink_metadata(path) {
            Ok(meta) => {
                if !meta.is_file() || qqnt_adapters::is_link(&meta) {
                    return Err("索引文件包含链接或不安全类型".into());
                }
                let f = match root.open(path) {
                    Ok(file) => file.into_std(),
                    // SQLite removes WAL/SHM/journal files when its last
                    // connection closes. Disappearance after metadata is a
                    // normal concurrent close, not a permission failure.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(format!(
                            "应用索引文件不可读（系统码 {:?}）",
                            error.raw_os_error()
                        ))
                    }
                };
                if qqnt_fs::fingerprint(&f)
                    .map_err(|_| "索引身份不可读取")?
                    .links
                    != 1
                {
                    return Err("索引文件状态变化或存在硬链接，拒绝修改".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("索引元数据不可读取".into()),
        }
    }
    Ok(())
}

impl Backend {
    fn publish_summary(&self, summary: &ScanSummary, detection: &Detection) {
        if summary.cancelled {
            return;
        }
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let parent = self
                .db
                .parent()
                .ok_or_else(|| std::io::Error::other("missing data directory"))?;
            let (_, root) = open_authorized(parent)?;
            let name = format!("summary-{}.tmp", Uuid::new_v4());
            let mut options = cap_std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = root.open_with(&name, &options)?;
            let bytes=serde_json::to_vec(&serde_json::json!({"schema":1,"version":env!("CARGO_PKG_VERSION"),"totalBytes":summary.progress.bytes,"files":summary.progress.files,"scannedAt":chrono::Utc::now().timestamp(),"recognized":detection.recognized})).map_err(std::io::Error::other)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            root.rename(&name, &root, "public-summary.json")?;
            Ok(())
        })();
        if result.is_err() {
            tracing::warn!("public scan summary unavailable");
        }
    }
}

pub(crate) struct MutationGuard<'a> {
    backend: &'a Backend,
}
impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut a) = self.backend.activity.lock() {
            a.status.state = "idle".into();
        }
    }
}
