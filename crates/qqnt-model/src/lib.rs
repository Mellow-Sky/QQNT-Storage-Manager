use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MediaKind {
    Image,
    Video,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    SafeCandidate,
    ReviewRequired,
    Protected,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub adapter: String,
    pub version: String,
    pub recognized: bool,
    pub read_only: bool,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedFile {
    pub relative_path: String,
    pub directory: String,
    pub size: u64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub kind: MediaKind,
    pub risk: Risk,
    pub evidence: Vec<String>,
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
    #[serde(default)]
    pub user_protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub identity: String,
    pub size: u64,
    pub modified_nanos: Option<String>,
    pub created_nanos: Option<String>,
    pub change_time: Option<String>,
    pub links: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub count: u64,
    pub bytes: u64,
}

impl Bucket {
    pub fn add(&mut self, size: u64) {
        self.count += 1;
        self.bytes = self.bytes.saturating_add(size);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub files: u64,
    pub bytes: u64,
    pub directories: u64,
    pub skipped_links: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub reused_files: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub progress: ScanProgress,
    pub by_kind: BTreeMap<MediaKind, Bucket>,
    pub by_risk: BTreeMap<Risk, Bucket>,
    pub by_directory: BTreeMap<String, Bucket>,
    pub by_month: BTreeMap<String, Bucket>,
    pub warnings: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum FileSort {
    #[default]
    SizeDesc,
    SizeAsc,
    ModifiedDesc,
    ModifiedAsc,
    Path,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileQuery {
    pub session_id: String,
    pub kind: Option<MediaKind>,
    pub risk: Option<Risk>,
    pub search: String,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_from: Option<i64>,
    pub modified_to: Option<i64>,
    pub sort: FileSort,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePage {
    pub total: u64,
    #[serde(default)]
    pub protected_count: u64,
    pub files: Vec<IndexedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPreview {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub key: String,
    pub filename: String,
    pub size: u64,
    pub created: i64,
    pub last_access: i64,
    pub digest: String,
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSession {
    pub id: String,
    pub root: String,
    pub started: i64,
    pub finished: Option<i64>,
    pub status: String,
    pub detection: Detection,
    pub summary: Option<ScanSummary>,
}
