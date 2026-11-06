pub mod cleanup;
pub mod media;
pub mod policy;
pub mod service;

#[cfg(feature = "desktop")]
mod desktop {
    use super::service::{Backend, Grant, Result, Status};
    use qqnt_model::{FilePage, FileQuery, ScanSession};
    use std::sync::Arc;
    use tauri::{Manager, State};
    use tauri_plugin_dialog::DialogExt;

    #[tauri::command]
    async fn choose_directory(
        app: tauri::AppHandle,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<Vec<Grant>> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let Some(path) = app
                .dialog()
                .file()
                .set_title("选择 QQNT 数据目录")
                .blocking_pick_folder()
            else {
                return Ok(vec![]);
            };
            let path = path.into_path().map_err(|_| "请选择本地目录")?;
            b.choose(&path)
        })
        .await
        .map_err(|_| "目录选择任务失败")?
    }
    #[tauri::command]
    async fn discover_directories(backend: State<'_, Arc<Backend>>) -> Result<Vec<Grant>> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.discover())
            .await
            .map_err(|_| "自动发现失败".into())
    }
    #[tauri::command]
    async fn start_scan(grant_id: String, backend: State<'_, Arc<Backend>>) -> Result<String> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.start(&grant_id))
            .await
            .map_err(|_| "无法启动扫描")?
    }
    #[tauri::command]
    fn cancel_scan(session_id: String, backend: State<'_, Arc<Backend>>) -> Result<()> {
        backend.cancel(&session_id)
    }
    #[tauri::command]
    fn scan_status(backend: State<'_, Arc<Backend>>) -> Result<Status> {
        backend.status()
    }
    #[tauri::command]
    async fn list_sessions(backend: State<'_, Arc<Backend>>) -> Result<Vec<ScanSession>> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.sessions())
            .await
            .map_err(|_| "无法读取历史")?
    }
    #[tauri::command]
    async fn list_files(query: FileQuery, backend: State<'_, Arc<Backend>>) -> Result<FilePage> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.files(query))
            .await
            .map_err(|_| "无法查询索引")?
    }
    #[tauri::command]
    async fn media_preview(
        session_id: String,
        relative_path: String,
        full: bool,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_model::MediaPreview> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.preview(&session_id, &relative_path, full))
            .await
            .map_err(|_| "预览任务失败")?
    }
    #[tauri::command]
    async fn video_token(
        session_id: String,
        relative_path: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<String> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.video_token(&session_id, &relative_path))
            .await
            .map_err(|_| "视频授权失败")?
    }
    #[tauri::command]
    async fn protect_files(
        session_id: String,
        paths: Vec<String>,
        protected: bool,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<()> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.protect(&session_id, paths, protected))
            .await
            .map_err(|_| "保护任务失败")?
    }
    #[tauri::command]
    async fn cleanup_enabled(backend: State<'_, Arc<Backend>>) -> Result<bool> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.cleanup_enabled())
            .await
            .map_err(|_| "读取清理设置失败")?
    }
    #[tauri::command]
    async fn enable_cleanup(enabled: bool, backend: State<'_, Arc<Backend>>) -> Result<()> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.enable_cleanup(enabled))
            .await
            .map_err(|_| "清理设置失败")?
    }
    #[tauri::command]
    async fn plan_cleanup(
        request: crate::cleanup::PlanRequest,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_cleaner::CleanupPlan> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.plan_cleanup(request))
            .await
            .map_err(|_| "模拟任务失败")?
    }
    #[tauri::command]
    async fn execute_cleanup(
        plan_id: String,
        confirmation: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_cleaner::CleanupOperation> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.execute_cleanup(&plan_id, &confirmation))
            .await
            .map_err(|_| "清理任务失败")?
    }
    #[tauri::command]
    async fn quarantine_page(
        offset: u32,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<crate::cleanup::QuarantinePage> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.quarantine_page(offset))
            .await
            .map_err(|_| "隔离区读取失败")?
    }
    #[tauri::command]
    async fn quarantine_action(
        ids: Vec<String>,
        action: String,
        confirmation: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_cleaner::CleanupOperation> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || {
            b.quarantine_action(ids, &action, &confirmation)
        })
        .await
        .map_err(|_| "隔离区操作失败")?
    }
    #[tauri::command]
    async fn delete_files(
        session_id: String,
        paths: Vec<String>,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<crate::cleanup::DeleteResult> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.delete_files(session_id, paths))
            .await
            .map_err(|_| "删除任务失败")?
    }
    #[tauri::command]
    async fn delete_matching(
        query: FileQuery,
        excluded_paths: Vec<String>,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<crate::cleanup::DeleteResult> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.delete_matching(query, excluded_paths))
            .await
            .map_err(|_| "删除任务失败")?
    }
    #[tauri::command]
    async fn protect_matching(
        query: FileQuery,
        excluded_paths: Vec<String>,
        protected: bool,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<()> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || {
            b.protect_matching(query, excluded_paths, protected)
        })
        .await
        .map_err(|_| "保护操作失败")?
    }
    #[tauri::command]
    async fn get_session(
        session_id: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<ScanSession> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.session(&session_id))
            .await
            .map_err(|_| "读取索引失败")?
    }
    #[tauri::command]
    fn cancel_delete(backend: State<'_, Arc<Backend>>) -> Result<()> {
        backend.cancel_delete()
    }
    #[tauri::command]
    async fn restore_files(
        ids: Vec<String>,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_cleaner::CleanupOperation> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.restore_files(ids))
            .await
            .map_err(|_| "恢复任务失败")?
    }
    #[tauri::command]
    async fn cleanup_history(backend: State<'_, Arc<Backend>>) -> Result<Vec<serde_json::Value>> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.cleanup_history())
            .await
            .map_err(|_| "清理历史读取失败")?
    }
    #[tauri::command]
    async fn reveal_file(
        session_id: String,
        relative_path: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<()> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let target = b.reveal_target(&session_id, &relative_path)?;
            #[cfg(windows)]
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", target.display()))
                .spawn()
                .map_err(|_| "无法打开资源管理器")?;
            #[cfg(not(windows))]
            {
                let _ = target;
                return Err("资源管理器定位仅支持 Windows".into());
            }
            #[cfg(windows)]
            Ok(())
        })
        .await
        .map_err(|_| "定位任务失败")?
    }

    pub fn builder() -> tauri::Builder<tauri::Wry> {
        builder_with_media()
    }

    #[tauri::command]
    async fn policy_settings(
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_policy::PolicySettings> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.policy_settings())
            .await
            .map_err(|_| "策略设置读取失败")?
    }
    #[tauri::command]
    async fn save_policy(
        settings: qqnt_policy::PolicySettings,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<()> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.save_policy(settings))
            .await
            .map_err(|_| "策略保存失败")?
    }
    #[tauri::command]
    async fn policy_report(
        session_id: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_policy::PolicyReport> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.policy_report(&session_id))
            .await
            .map_err(|_| "策略分析失败")?
    }
    #[tauri::command]
    async fn clean_app_cache(
        all: bool,
        confirmation: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<qqnt_cleaner::CleanupOperation> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.clean_app_cache(all, &confirmation))
            .await
            .map_err(|_| "应用缓存清理失败")?
    }
    #[tauri::command]
    async fn disk_status(
        grant_id: String,
        backend: State<'_, Arc<Backend>>,
    ) -> Result<serde_json::Value> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.disk_status(&grant_id))
            .await
            .map_err(|_| "磁盘空间查询失败")?
    }

    #[tauri::command]
    async fn import_quarantine(grant_id: String, backend: State<'_, Arc<Backend>>) -> Result<u64> {
        let b = Arc::clone(backend.inner());
        tauri::async_runtime::spawn_blocking(move || b.import_quarantine(&grant_id))
            .await
            .map_err(|_| "隔离清单导入失败")?
    }

    fn builder_with_media() -> tauri::Builder<tauri::Wry> {
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .register_asynchronous_uri_scheme_protocol(
                "qqnt-media",
                |context, request, responder| {
                    if context.webview_label() != "main" {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(403)
                                .body(Vec::new())
                                .expect("static response"),
                        );
                        return;
                    }
                    let backend = context.app_handle().state::<Arc<Backend>>().inner().clone();
                    let token = request.uri().path().trim_start_matches('/').to_string();
                    let range = request
                        .headers()
                        .get("Range")
                        .and_then(|h| h.to_str().ok())
                        .map(str::to_owned);
                    if request.method() != "GET" && request.method() != "HEAD" {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(405)
                                .body(Vec::new())
                                .expect("static response"),
                        );
                        return;
                    }
                    let head = request.method() == "HEAD";
                    tauri::async_runtime::spawn_blocking(move || {
                        let response = match backend.media.video_chunk(&token, range.as_deref()) {
                            Ok(chunk) => tauri::http::Response::builder()
                                .status(206)
                                .header("Content-Type", chunk.mime)
                                .header("Accept-Ranges", "bytes")
                                .header("Cache-Control", "no-store")
                                .header("X-Content-Type-Options", "nosniff")
                                .header(
                                    "Content-Range",
                                    format!("bytes {}-{}/{}", chunk.start, chunk.end, chunk.total),
                                )
                                .header("Content-Length", chunk.bytes.len().to_string())
                                .body(if head { Vec::new() } else { chunk.bytes }),
                            Err(_) => tauri::http::Response::builder()
                                .status(416)
                                .header("Cache-Control", "no-store")
                                .body(Vec::new()),
                        };
                        if let Ok(response) = response {
                            responder.respond(response);
                        }
                    });
                },
            )
            .invoke_handler(tauri::generate_handler![
                choose_directory,
                discover_directories,
                start_scan,
                cancel_scan,
                scan_status,
                list_sessions,
                list_files,
                media_preview,
                video_token,
                protect_files,
                cleanup_enabled,
                enable_cleanup,
                plan_cleanup,
                execute_cleanup,
                delete_files,
                delete_matching,
                protect_matching,
                get_session,
                cancel_delete,
                restore_files,
                quarantine_page,
                import_quarantine,
                quarantine_action,
                cleanup_history,
                policy_settings,
                save_policy,
                policy_report,
                clean_app_cache,
                disk_status,
                reveal_file
            ])
    }

    pub fn run() {
        tracing_subscriber::fmt()
            .with_target(false)
            .with_env_filter("info")
            .init();
        builder()
            .setup(|app| {
                let path = app.path().app_local_data_dir()?;
                let backend = Backend::new(&path).map_err(std::io::Error::other)?;
                backend.launch_scheduler();
                app.manage(backend);
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("桌面运行时初始化失败");
    }
}
#[cfg(feature = "desktop")]
pub use desktop::{builder, run};
