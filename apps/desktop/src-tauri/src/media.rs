use crate::service::Result;
use cap_std::fs::{Dir, OpenOptions};
use qqnt_indexer::Index;
use qqnt_model::{CacheEntry, IndexedFile, MediaPreview};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

struct MemoryCache {
    items: HashMap<String, MediaPreview>,
    order: VecDeque<String>,
    bytes: usize,
}
#[derive(Clone)]
struct VideoLease {
    root: Arc<Dir>,
    file: IndexedFile,
    expires: Instant,
}
pub struct MediaService {
    cache: Mutex<MemoryCache>,
    videos: Mutex<HashMap<String, VideoLease>>,
    disk: Dir,
    disk_path: std::path::PathBuf,
}

impl MediaService {
    pub fn new(data: &Path) -> Result<Self> {
        let path = data.join("thumbnails");
        std::fs::create_dir_all(&path).map_err(|_| "缩略图目录创建失败")?;
        let (disk_path, disk) =
            qqnt_adapters::open_authorized(&path).map_err(|_| "缩略图目录包含不安全链接")?;
        Ok(Self {
            cache: Mutex::new(MemoryCache {
                items: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
            }),
            videos: Mutex::new(HashMap::new()),
            disk,
            disk_path,
        })
    }
    pub fn preview(
        &self,
        index: &Index,
        root: Arc<Dir>,
        root_path: &str,
        file: &IndexedFile,
        full: bool,
    ) -> Result<MediaPreview> {
        // The mutex also bounds decoding to one image and 128MiB decoder allocation.
        let mut cache = self.cache.lock().map_err(|_| "预览队列不可用")?;
        drop(qqnt_media::open_indexed(&root, file).map_err(|e| e.to_string())?);
        let key = blake3::hash(
            format!(
                "{root_path}\0{}\0{}",
                file.relative_path,
                serde_json::to_string(&file.fingerprint).map_err(|_| "身份格式无效")?
            )
            .as_bytes(),
        )
        .to_hex()
        .to_string();
        if !full {
            if let Some(value) = cache.items.get(&key) {
                return Ok(value.clone());
            }
            if let Some(entry) = index.cache_entry(&key).map_err(|e| e.to_string())? {
                if valid_filename(&entry.filename)
                    && qqnt_adapters::reject_links(&self.disk, Path::new(&entry.filename)).is_ok()
                {
                    if let Ok(opened) = self.disk.open(&entry.filename) {
                        if opened.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024) {
                            let mut buf = Vec::new();
                            if opened
                                .take(4 * 1024 * 1024 + 1)
                                .read_to_end(&mut buf)
                                .is_ok()
                                && blake3::hash(&buf).to_hex().as_str() == entry.digest
                            {
                                if let Ok(value) = serde_json::from_slice::<MediaPreview>(&buf) {
                                    let _ = index.touch_cache(&key);
                                    put_memory(&mut cache, key, value.clone());
                                    return Ok(value);
                                }
                            }
                        }
                    }
                }
            }
        }
        let preview = qqnt_media::image_preview(&root, file, full).map_err(|e| e.to_string())?;
        if !full {
            if index
                .setting::<bool>("media_cache_enabled")
                .map_err(|e| e.to_string())?
                .unwrap_or(false)
            {
                let bytes = serde_json::to_vec(&preview).map_err(|_| "预览编码失败")?;
                let filename = format!("{}.preview", Uuid::new_v4());
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                if let Ok(mut output) = self.disk.open_with(&filename, &options) {
                    if output.write_all(&bytes).is_ok() {
                        drop(output);
                        let fingerprint = self
                            .disk
                            .open(&filename)
                            .ok()
                            .and_then(|f| qqnt_fs::fingerprint(&f.into_std()).ok());
                        let now = chrono::Utc::now().timestamp();
                        let entry = CacheEntry {
                            key: key.clone(),
                            filename,
                            size: bytes.len() as u64,
                            created: now,
                            last_access: now,
                            digest: blake3::hash(&bytes).to_hex().to_string(),
                            fingerprint,
                        };
                        let _ = index.save_cache(&entry);
                    }
                }
            }
            put_memory(&mut cache, key, preview.clone());
        }
        Ok(preview)
    }
    pub fn token(&self, root: Arc<Dir>, file: IndexedFile) -> Result<String> {
        if file.kind != qqnt_model::MediaKind::Video {
            return Err("不是视频文件".into());
        }
        drop(qqnt_media::open_indexed(&root, &file).map_err(|e| e.to_string())?);
        let mut videos = self.videos.lock().map_err(|_| "视频会话不可用")?;
        videos.retain(|_, v| v.expires > Instant::now());
        if videos.len() >= 32 {
            return Err("视频会话过多，请稍后重试".into());
        }
        let token = Uuid::new_v4().to_string();
        videos.insert(
            token.clone(),
            VideoLease {
                root,
                file,
                expires: Instant::now() + Duration::from_secs(900),
            },
        );
        Ok(token)
    }
    pub fn video_chunk(&self, token: &str, range: Option<&str>) -> Result<qqnt_media::VideoChunk> {
        let lease = self
            .videos
            .lock()
            .map_err(|_| "视频会话不可用")?
            .get(token)
            .cloned()
            .ok_or("无效的视频会话")?;
        if lease.expires < Instant::now() {
            return Err("视频会话已过期，请重新打开预览".into());
        }
        qqnt_media::video_chunk(&lease.root, &lease.file, range).map_err(|e| e.to_string())
    }
    pub fn disk(&self) -> &Dir {
        &self.disk
    }
    pub fn disk_path(&self) -> &Path {
        &self.disk_path
    }
    pub fn clear_memory(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.items.clear();
            cache.order.clear();
            cache.bytes = 0;
        }
    }
}

pub fn valid_filename(name: &str) -> bool {
    name.strip_suffix(".preview")
        .is_some_and(|id| Uuid::parse_str(id).is_ok())
}
fn put_memory(cache: &mut MemoryCache, key: String, value: MediaPreview) {
    while cache.bytes + value.data_url.len() > 32 * 1024 * 1024 || cache.items.len() >= 96 {
        let Some(old) = cache.order.pop_front() else {
            break;
        };
        if let Some(value) = cache.items.remove(&old) {
            cache.bytes = cache.bytes.saturating_sub(value.data_url.len());
        }
    }
    cache.bytes += value.data_url.len();
    cache.order.push_back(key.clone());
    cache.items.insert(key, value);
}
