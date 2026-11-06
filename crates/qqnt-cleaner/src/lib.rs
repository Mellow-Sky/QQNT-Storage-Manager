//! Dry-run planning is portable. QQ mutations use Windows handle APIs only.
use cap_std::fs::Dir;
use qqnt_model::{Fingerprint, IndexedFile, Risk};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Seek},
    path::Path,
};
use uuid::Uuid;
pub const VAULT: &str = ".qqnt-storage-manager-quarantine";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CleanupMode {
    Conservative,
    Standard,
    Advanced,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRule {
    pub min_age_days: Option<u32>,
    pub min_size: Option<u64>,
    pub extensions: Vec<String>,
}
impl CleanupRule {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_age_days.is_some_and(|d| d > 36500)
            || self.extensions.len() > 32
            || self.extensions.iter().any(|e| {
                e.is_empty() || e.len() > 16 || !e.bytes().all(|b| b.is_ascii_alphanumeric())
            })
        {
            return Err("自定义规则无效：天数最大 36500，最多 32 个字母数字扩展名".into());
        }
        Ok(())
    }
    pub fn matches(&self, file: &IndexedFile, now: i64) -> bool {
        if self.min_size.is_some_and(|size| file.size < size) {
            return false;
        }
        if let Some(days) = self.min_age_days {
            if !file
                .modified
                .is_some_and(|t| t <= now.saturating_sub(days as i64 * 86400))
            {
                return false;
            }
        }
        self.extensions.is_empty()
            || Path::new(&file.relative_path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    self.extensions
                        .iter()
                        .any(|expected| e.eq_ignore_ascii_case(expected))
                })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub file: IndexedFile,
    pub allowed: bool,
    pub reason: String,
    pub digest: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub id: String,
    pub session_id: String,
    pub root: String,
    pub adapter_version: String,
    pub created: i64,
    pub expires: i64,
    pub mode: CleanupMode,
    pub media_review: bool,
    pub entries: Vec<PlanEntry>,
    pub files: u64,
    pub bytes: u64,
    pub state: String,
    #[serde(default)]
    pub rules: Option<CleanupRule>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineItem {
    pub id: String,
    pub plan_id: String,
    pub root: String,
    pub relative_path: String,
    pub stored_name: String,
    pub size: u64,
    pub digest: String,
    pub fingerprint: Fingerprint,
    pub quarantined_at: i64,
    pub state: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFile {
    pub relative_path: String,
    pub state: String,
    pub reason: String,
    pub quarantine_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOperation {
    pub id: String,
    pub plan_id: String,
    pub action: String,
    pub started: i64,
    pub finished: Option<i64>,
    pub files: Vec<OperationFile>,
    pub succeeded: u64,
    pub failed: u64,
    pub logical_bytes: u64,
}

pub fn eligible(
    file: &IndexedFile,
    recognized: bool,
    mode: CleanupMode,
    media_review: bool,
) -> Result<(), String> {
    if !recognized {
        return Err("未知布局只允许读取".into());
    }
    if !qqnt_adapters::safe_relative(Path::new(&file.relative_path))
        || file.relative_path.split('/').any(|c| c == VAULT)
    {
        return Err("路径不在允许的媒体范围".into());
    }
    if qqnt_classifier::protected_path(Path::new(&file.relative_path))
        || file.user_protected
        || file.risk == Risk::Protected
    {
        return Err("拒绝优先：数据库、配置、收藏、接收文件或用户保护".into());
    }
    if file.risk == Risk::Unknown {
        return Err("证据不足的文件禁止清理".into());
    }
    if file.risk == Risk::ReviewRequired && (mode == CleanupMode::Conservative || !media_review) {
        return Err("媒体原件必须在标准/高级模式逐项审核并接受不可恢复风险".into());
    }
    let fp = file
        .fingerprint
        .as_ref()
        .ok_or("缺少扫描文件身份，请重新扫描")?;
    if fp.links != 1 {
        return Err("硬链接文件禁止清理".into());
    }
    Ok(())
}

// These explicit inputs keep authorization and review consent at the call site.
#[allow(clippy::too_many_arguments)]
pub fn dry_run(
    root: &Dir,
    session_id: &str,
    root_path: &str,
    adapter_version: &str,
    recognized: bool,
    mode: CleanupMode,
    media_review: bool,
    files: Vec<IndexedFile>,
) -> CleanupPlan {
    let now = chrono::Utc::now().timestamp();
    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut count = 0;
    for file in files {
        let result = (|| -> Result<String, String> {
            eligible(&file, recognized, mode, media_review)?;
            let mut opened = open_indexed(root, &file).map_err(|e| e.to_string())?;
            if opened
                .metadata()
                .map_err(|e| e.to_string())?
                .permissions()
                .readonly()
            {
                return Err("文件带有只读保护属性".into());
            }
            let mut header = [0u8; 32];
            let n = opened.read(&mut header).map_err(|e| e.to_string())?;
            opened.rewind().map_err(|e| e.to_string())?;
            let (_, risk, _) =
                qqnt_classifier::classify(Path::new(&file.relative_path), &header[..n], recognized);
            if risk == Risk::Protected || risk == Risk::Unknown {
                return Err("重新分类：受保护或证据不足".into());
            }
            let mut fresh = file.clone();
            fresh.risk = risk;
            eligible(&fresh, recognized, mode, media_review)?;
            let before = qqnt_fs::fingerprint(&opened).map_err(|e| e.to_string())?;
            let digest = qqnt_fs::content_hash(&mut opened).map_err(|e| e.to_string())?;
            if before != qqnt_fs::fingerprint(&opened).map_err(|e| e.to_string())? {
                return Err("生成计划期间文件已变化".into());
            }
            Ok(digest)
        })();
        match result {
            Ok(digest) => {
                count += 1;
                bytes += file.size;
                entries.push(PlanEntry {
                    file,
                    allowed: true,
                    reason: "已校验身份和内容摘要；仅可在 QQ 完全退出后按计划执行".into(),
                    digest: Some(digest),
                });
            }
            Err(reason) => entries.push(PlanEntry {
                file,
                allowed: false,
                reason,
                digest: None,
            }),
        }
    }
    CleanupPlan {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.into(),
        root: root_path.into(),
        adapter_version: adapter_version.into(),
        created: now,
        expires: now + 900,
        mode,
        media_review,
        entries,
        files: count,
        bytes,
        state: "draft".into(),
        rules: None,
    }
}

fn open_indexed(root: &Dir, file: &IndexedFile) -> io::Result<std::fs::File> {
    let relative = Path::new(&file.relative_path);
    qqnt_adapters::reject_links(root, relative)?;
    let opened = root.open(relative)?.into_std();
    let fp = qqnt_fs::fingerprint(&opened)?;
    if file.fingerprint.as_ref() != Some(&fp) {
        return Err(io::Error::other("文件身份、大小或时间已变化"));
    }
    Ok(opened)
}

pub fn validate_vault(root: &Dir, owner: &str, create: bool) -> io::Result<()> {
    if Uuid::parse_str(owner).is_err() {
        return Err(io::Error::other("invalid vault owner"));
    }
    let created = if create {
        match root.create_dir(VAULT) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
            Err(e) => return Err(e),
        }
    } else {
        false
    };
    qqnt_adapters::reject_links(root, Path::new(VAULT))?;
    let vault = root.open_dir(VAULT)?;
    if created {
        let mut o = cap_std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        use std::io::Write;
        let mut marker = vault.open_with("owner", &o)?;
        marker.write_all(owner.as_bytes())?;
        marker.sync_all()?;
    }
    qqnt_adapters::reject_links(&vault, Path::new("owner"))?;
    let mut marker = String::new();
    vault.open("owner")?.take(128).read_to_string(&mut marker)?;
    if marker != owner {
        return Err(io::Error::other("隔离区目录不属于此应用索引，拒绝操作"));
    }
    Ok(())
}

pub fn pending_item(plan: &CleanupPlan, entry: &PlanEntry) -> Result<QuarantineItem, String> {
    let id = Uuid::new_v4().to_string();
    Ok(QuarantineItem {
        id: id.clone(),
        plan_id: plan.id.clone(),
        root: plan.root.clone(),
        relative_path: entry.file.relative_path.clone(),
        stored_name: format!("{id}.bin"),
        size: entry.file.size,
        digest: entry.digest.clone().ok_or("计划未校验内容")?,
        fingerprint: entry.file.fingerprint.clone().ok_or("缺少文件身份")?,
        quarantined_at: chrono::Utc::now().timestamp(),
        state: "pending".into(),
        error: None,
    })
}

pub fn write_manifest(root: &Dir, item: &QuarantineItem, initial: bool) -> io::Result<()> {
    if Uuid::parse_str(&item.id).is_err() {
        return Err(io::Error::other("invalid manifest identifier"));
    }
    qqnt_adapters::reject_links(root, Path::new(VAULT))?;
    let vault = root.open_dir(VAULT)?;
    let target = format!("{}.json", item.id);
    let name = if initial {
        target.clone()
    } else {
        format!("{}.tmp", Uuid::new_v4())
    };
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = vault.open_with(&name, &options)?;
    use std::io::Write;
    file.write_all(
        &serde_json::to_vec(&serde_json::json!({"schema":1,"item":item}))
            .map_err(io::Error::other)?,
    )?;
    file.sync_all()?;
    drop(file);
    if !initial {
        vault.rename(&name, &vault, &target)?;
    }
    Ok(())
}

#[cfg(windows)]
pub mod mutation;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_run_never_modifies_and_denies_protected_or_unknown() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("chat.db"), b"SQLite format 3\0")?;
        let (_, root) = qqnt_adapters::open_authorized(temp.path())?;
        let f = IndexedFile {
            relative_path: "chat.db".into(),
            directory: ".".into(),
            size: 16,
            created: None,
            modified: None,
            kind: qqnt_model::MediaKind::Other,
            risk: Risk::Protected,
            evidence: vec![],
            fingerprint: None,
            user_protected: false,
        };
        let plan = dry_run(
            &root,
            "s",
            "root",
            "1",
            true,
            CleanupMode::Advanced,
            true,
            vec![f],
        );
        assert_eq!(plan.files, 0);
        assert!(!plan.entries[0].allowed);
        assert_eq!(
            std::fs::read(temp.path().join("chat.db"))?,
            b"SQLite format 3\0"
        );
        Ok(())
    }
}
