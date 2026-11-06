use qqnt_model::{CacheEntry, IndexedFile, Risk};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PolicySettings {
    pub retention_days: Option<u32>,
    pub scope_limit_bytes: Option<u64>,
    pub app_cache_limit_bytes: u64,
    pub low_disk_bytes: u64,
    pub auto_cleanup: bool,
    pub auto_scan: bool,
    pub scan_interval_minutes: u32,
    pub disk_cache_enabled: bool,
}
impl Default for PolicySettings {
    fn default() -> Self {
        Self {
            retention_days: Some(90),
            scope_limit_bytes: None,
            app_cache_limit_bytes: 256 * 1024 * 1024,
            low_disk_bytes: 5 * 1024 * 1024 * 1024,
            auto_cleanup: false,
            auto_scan: false,
            scan_interval_minutes: 60,
            disk_cache_enabled: false,
        }
    }
}
impl PolicySettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.retention_days.is_some_and(|d| d == 0 || d > 36500)
            || self.app_cache_limit_bytes > 10 * 1024 * 1024 * 1024
            || self.app_cache_limit_bytes < 1024 * 1024
            || self.scan_interval_minutes < 5
            || self.scan_interval_minutes > 1440
        {
            return Err(
                "策略范围无效：保留 1–36500 天，应用缓存 1MiB–10GiB，自动扫描间隔 5–1440 分钟"
                    .into(),
            );
        }
        Ok(())
    }
}

pub fn review_candidate(file: &IndexedFile, settings: &PolicySettings, now: i64) -> bool {
    file.risk == Risk::ReviewRequired
        && !file.user_protected
        && settings.retention_days.is_some_and(|days| {
            file.modified
                .is_some_and(|t| t <= now - days as i64 * 86400)
        })
}

pub fn cache_evictions(
    entries: &[CacheEntry],
    settings: &PolicySettings,
    now: i64,
) -> Vec<CacheEntry> {
    let mut ordered = entries.to_vec();
    ordered.sort_by_key(|e| e.last_access);
    let mut remaining: u64 = ordered.iter().map(|e| e.size).sum();
    let mut result = Vec::new();
    for entry in ordered {
        let expired = settings
            .retention_days
            .is_some_and(|days| entry.last_access <= now - days as i64 * 86400);
        if expired || remaining > settings.app_cache_limit_bytes {
            remaining = remaining.saturating_sub(entry.size);
            result.push(entry);
        }
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyReport {
    pub review_count: u64,
    pub review_bytes: u64,
    pub review_paths: Vec<String>,
    pub scope_bytes: u64,
    pub scope_over_limit: bool,
    pub available_bytes: Option<u64>,
    pub low_disk: bool,
    pub cache_count: u64,
    pub cache_bytes: u64,
    pub cache_reclaimable_bytes: u64,
    pub warning: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_derived_cache_is_automatically_evicted() {
        let policy = PolicySettings {
            app_cache_limit_bytes: 10,
            retention_days: None,
            ..Default::default()
        };
        let data = vec![
            CacheEntry {
                key: "old".into(),
                filename: "old".into(),
                size: 7,
                created: 1,
                last_access: 1,
                digest: "a".into(),
                fingerprint: None,
            },
            CacheEntry {
                key: "new".into(),
                filename: "new".into(),
                size: 7,
                created: 2,
                last_access: 2,
                digest: "b".into(),
                fingerprint: None,
            },
        ];
        let evictions = cache_evictions(&data, &policy, 100);
        assert_eq!(evictions.len(), 1);
        assert_eq!(evictions[0].key, "old");
    }
}
