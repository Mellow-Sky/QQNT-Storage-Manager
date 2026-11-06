use qqnt_model::{Detection, FilePage, FileQuery, FileSort, IndexedFile, ScanSession, ScanSummary};
use rusqlite::{params, types::Value, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("索引数据库操作失败")]
    Sql(#[from] rusqlite::Error),
    #[error("索引数据格式无效")]
    Json(#[from] serde_json::Error),
    #[error("索引架构版本比应用更新，拒绝打开")]
    FutureSchema,
    #[error("选择中包含不在扫描索引中的文件")]
    InvalidSelection,
}
pub type Result<T> = std::result::Result<T, Error>;
type SessionRow = (String, i64, Option<i64>, String, String, Option<String>);

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn latest_scan_time(&self, root: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT max(started) FROM scan_sessions WHERE root=?1",
            [root],
            |r| r.get(0),
        )?)
    }
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        let mut index = Self { conn };
        index.migrate()?;
        Ok(index)
    }
    fn migrate(&mut self) -> Result<()> {
        let version: u32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 4 {
            return Err(Error::FutureSchema);
        }
        if version == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.commit()?;
        }
        if version < 2 {
            self.conn.execute_batch(include_str!("migration-2.sql"))?;
        }
        if version < 3 {
            self.conn.execute_batch("BEGIN IMMEDIATE; ALTER TABLE media_cache ADD COLUMN payload TEXT; PRAGMA user_version=3; COMMIT;")?;
        }
        if version < 4 {
            self.conn.execute_batch(include_str!("migration-4.sql"))?;
        }
        Ok(())
    }
    pub fn begin(&self, id: &str, root: &str, detection: &Detection) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO accounts(root,adapter_version) VALUES(?1,?2)",
            params![root, detection.version],
        )?;
        self.conn.execute("INSERT INTO scan_sessions(id,root,started,status,detection) VALUES(?1,?2,?3,'running',?4)", params![id, root, chrono::Utc::now().timestamp(), serde_json::to_string(detection)?])?;
        Ok(())
    }
    pub fn append(&mut self, id: &str, files: &[IndexedFile]) -> Result<()> {
        let (root, detection): (String, String) = self.conn.query_row(
            "SELECT root,detection FROM scan_sessions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let tag = cache_tag(&serde_json::from_str::<Detection>(&detection)?);
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached("INSERT OR REPLACE INTO indexed_files(session_id,path,kind,risk,size,modified,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)")?;
            let mut classify = tx.prepare_cached("INSERT OR REPLACE INTO classification_results(session_id,path,risk,evidence) VALUES(?1,?2,?3,?4)")?;
            let mut cache=tx.prepare_cached("INSERT OR REPLACE INTO scan_cache(root,path,adapter,fingerprint,payload) VALUES(?1,?2,?3,?4,?5)")?;
            for f in files {
                insert.execute(params![
                    id,
                    f.relative_path,
                    format!("{:?}", f.kind),
                    format!("{:?}", f.risk),
                    f.size.min(i64::MAX as u64) as i64,
                    f.modified,
                    serde_json::to_string(f)?
                ])?;
                classify.execute(params![
                    id,
                    f.relative_path,
                    format!("{:?}", f.risk),
                    serde_json::to_string(&f.evidence)?
                ])?;
                if let Some(fp) = &f.fingerprint {
                    if !f.evidence.iter().any(|e| e.contains("读取失败")) {
                        cache.execute(params![
                            root,
                            f.relative_path,
                            tag,
                            serde_json::to_string(fp)?,
                            serde_json::to_string(f)?
                        ])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn finish(&self, id: &str, summary: &ScanSummary) -> Result<()> {
        self.conn.execute(
            "UPDATE scan_sessions SET finished=?1,status=?2,summary=?3 WHERE id=?4",
            params![
                chrono::Utc::now().timestamp(),
                if summary.cancelled {
                    "cancelled"
                } else {
                    "completed"
                },
                serde_json::to_string(summary)?,
                id
            ],
        )?;
        if !summary.cancelled {
            self.conn.execute("DELETE FROM scan_cache WHERE root=(SELECT root FROM scan_sessions WHERE id=?1) AND path NOT IN(SELECT path FROM indexed_files WHERE session_id=?1)",[id])?;
        }
        Ok(())
    }
    pub fn fail(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scan_sessions SET finished=?1,status='failed' WHERE id=?2",
            params![chrono::Utc::now().timestamp(), id],
        )?;
        Ok(())
    }
    pub fn recover_interrupted(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE scan_sessions SET status='interrupted',finished=?1 WHERE status='running'",
            [chrono::Utc::now().timestamp()],
        )?;
        // JSON payloads and SQL status must agree after an interrupted process.
        let now = chrono::Utc::now().timestamp();
        self.conn.execute("UPDATE cleanup_operations SET status='interrupted',payload=json_set(payload,'$.finished',?1) WHERE status='running'", [now])?;
        self.conn.execute("UPDATE cleanup_plans SET payload=json_set(payload,'$.state','interrupted') WHERE json_extract(payload,'$.state')='running'", [])?;
        Ok(())
    }
    pub fn sessions(&self) -> Result<Vec<ScanSession>> {
        let mut stmt = self.conn.prepare("SELECT id,root,started,finished,status,detection,summary FROM scan_sessions ORDER BY started DESC,rowid DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        rows.enumerate()
            .map(|(position, r)| {
                let (id, root, started, finished, status, detection, summary) = r?;
                let mut summary: Option<ScanSummary> =
                    summary.map(|s| serde_json::from_str(&s)).transpose()?;
                if position == 0 {
                    if let Some(value) = summary.as_mut() {
                        self.overlay_summary(&id, &root, value)?;
                    }
                }
                Ok(ScanSession {
                    id,
                    root,
                    started,
                    finished,
                    status,
                    detection: serde_json::from_str(&detection)?,
                    summary,
                })
            })
            .collect()
    }
    pub fn files(&self, query: &FileQuery) -> Result<FilePage> {
        let protected =
            "EXISTS(SELECT 1 FROM protected_items p WHERE p.root=s.root AND p.path=f.path)";
        let (sql, mut values) = file_conditions(query);
        let (total, protected_count): (i64, i64) = self.conn.query_row(
            &format!("SELECT count(*),coalesce(sum({protected}),0) FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id{sql}"),
            rusqlite::params_from_iter(&values), |r| Ok((r.get(0)?,r.get(1)?)),
        )?;
        let order = match query.sort {
            FileSort::SizeDesc => "size DESC",
            FileSort::SizeAsc => "size ASC",
            FileSort::ModifiedDesc => "modified DESC",
            FileSort::ModifiedAsc => "modified ASC",
            FileSort::Path => "path ASC",
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.payload,{protected} FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id{sql} ORDER BY {order},path ASC LIMIT ? OFFSET ?"
        ))?;
        values.push(Value::Integer(query.limit.clamp(1, 200) as i64));
        values.push(Value::Integer(query.offset as i64));
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        })?;
        let files = rows
            .map(|r| {
                let (raw, protected) = r?;
                let mut f: IndexedFile = serde_json::from_str(&raw)?;
                overlay(&mut f, protected);
                Ok(f)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(FilePage {
            total: total as u64,
            protected_count: protected_count as u64,
            files,
        })
    }
    pub fn file(&self, session: &str, path: &str) -> Result<Option<IndexedFile>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                &format!("SELECT f.payload FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id WHERE f.session_id=?1 AND f.path=?2 AND {PRESENT}"),
                params![session, path],
                |r| r.get(0),
            )
            .optional()?;
        let mut item: Option<IndexedFile> = raw
            .map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()?;
        if let Some(f) = item.as_mut() {
            let protected:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM protected_items p JOIN scan_sessions s ON s.root=p.root WHERE s.id=?1 AND p.path=?2)",params![session,path],|r|r.get(0))?;
            overlay(f, protected);
        }
        Ok(item)
    }

    pub fn session_metadata(&self, id: &str) -> Result<Option<ScanSession>> {
        let raw: Option<(String, i64, Option<i64>, String, String)> = self
            .conn
            .query_row(
                "SELECT root,started,finished,status,detection FROM scan_sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        raw.map(|(root, started, finished, status, detection)| {
            Ok(ScanSession {
                id: id.into(),
                root,
                started,
                finished,
                status,
                detection: serde_json::from_str(&detection)?,
                summary: None,
            })
        })
        .transpose()
    }
    pub fn session(&self, id: &str) -> Result<Option<ScanSession>> {
        // Session lookups must not be limited by the 100-row history UI.
        let raw: Option<SessionRow> = self.conn.query_row("SELECT root,started,finished,status,detection,summary FROM scan_sessions WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        raw.map(|(root, started, finished, status, detection, summary)| {
            let mut summary: Option<ScanSummary> =
                summary.map(|s| serde_json::from_str(&s)).transpose()?;
            if let Some(value) = summary.as_mut() {
                self.overlay_summary(id, &root, value)?;
            }
            Ok(ScanSession {
                id: id.into(),
                root,
                started,
                finished,
                status,
                detection: serde_json::from_str(&detection)?,
                summary,
            })
        })
        .transpose()
    }
    pub fn protect(&mut self, root: &str, paths: &[String], value: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        for path in paths {
            if value {
                tx.execute(
                    "INSERT OR IGNORE INTO protected_items(root,path) VALUES(?1,?2)",
                    params![root, path],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM protected_items WHERE root=?1 AND path=?2",
                    params![root, path],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn capture_query(&mut self, query: &FileQuery, excluded: &[String]) -> Result<u64> {
        self.reset_selection()?;
        let (conditions, values) = file_conditions(query);
        self.conn.execute(&format!("INSERT INTO selected_paths SELECT f.path FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id{conditions}"),rusqlite::params_from_iter(values))?;
        let tx = self.conn.transaction()?;
        {
            let mut statement = tx.prepare_cached("DELETE FROM selected_paths WHERE path=?1")?;
            for path in excluded {
                statement.execute([path])?;
            }
        }
        tx.commit()?;
        self.selection_count()
    }
    pub fn capture_paths(&mut self, session: &str, paths: &[String]) -> Result<u64> {
        self.reset_selection()?;
        let tx = self.conn.transaction()?;
        {
            let mut statement =
                tx.prepare_cached("INSERT OR IGNORE INTO selected_paths(path) VALUES(?1)")?;
            for path in paths {
                statement.execute([path])?;
            }
        }
        let invalid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM selected_paths t LEFT JOIN indexed_files f ON f.session_id=?1 AND f.path=t.path WHERE f.path IS NULL)",[session],|r|r.get(0))?;
        if invalid {
            return Err(Error::InvalidSelection);
        }
        tx.execute("DELETE FROM selected_paths WHERE path IN (SELECT f.path FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id JOIN file_removals r ON r.root=s.root AND r.path=f.path AND r.fingerprint=json_extract(f.payload,'$.fingerprint') WHERE f.session_id=?1)",[session])?;
        tx.commit()?;
        self.selection_count()
    }
    fn reset_selection(&self) -> Result<()> {
        self.conn.execute_batch("PRAGMA temp_store=FILE; DROP TABLE IF EXISTS temp.selected_paths; CREATE TEMP TABLE selected_paths(path TEXT PRIMARY KEY) WITHOUT ROWID;")?;
        Ok(())
    }
    fn selection_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM selected_paths", [], |r| {
                r.get::<_, i64>(0)
            })? as u64)
    }
    pub fn selection_batch(&self, after: &str, limit: u32) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT path FROM selected_paths WHERE path>?1 ORDER BY path LIMIT ?2",
        )?;
        let values = statement
            .query_map(params![after, limit.clamp(1, 200)], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(values)
    }
    pub fn protect_captured(&mut self, root: &str, value: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        if value {
            tx.execute("INSERT OR IGNORE INTO protected_items(root,path) SELECT ?1,path FROM selected_paths",[root])?;
        } else {
            tx.execute("DELETE FROM protected_items WHERE root=?1 AND path IN(SELECT path FROM selected_paths)",[root])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn save_quarantine_effect<T: serde::Serialize>(
        &self,
        id: &str,
        root: &str,
        state: &str,
        value: &T,
        file: &IndexedFile,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO quarantine_items(id,root,state,payload) VALUES(?1,?2,?3,?4)",
            params![id, root, state, serde_json::to_string(value)?],
        )?;
        if let Some(fp) = &file.fingerprint {
            let fingerprint = serde_json::to_string(fp)?;
            tx.execute("INSERT OR IGNORE INTO file_removals(root,path,fingerprint,removed_at) VALUES(?1,?2,?3,?4)",params![root,file.relative_path,fingerprint,chrono::Utc::now().timestamp()])?;
            tx.execute(
                "DELETE FROM scan_cache WHERE root=?1 AND path=?2 AND fingerprint=?3",
                params![root, file.relative_path, fingerprint],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn is_protected(&self, root: &str, path: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM protected_items WHERE root=?1 AND path=?2)",
            params![root, path],
            |r| r.get(0),
        )?)
    }
    pub fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
    pub fn save_plan<T: serde::Serialize>(&self, id: &str, plan: &T) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cleanup_plans(id,created,payload) VALUES(?1,?2,?3)",
            params![
                id,
                chrono::Utc::now().timestamp(),
                serde_json::to_string(plan)?
            ],
        )?;
        Ok(())
    }
    pub fn plan<T: serde::de::DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT payload FROM cleanup_plans WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn save_operation<T: serde::Serialize>(
        &self,
        id: &str,
        plan: &str,
        status: &str,
        value: &T,
    ) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO cleanup_operations(id,plan_id,status,payload) VALUES(?1,?2,?3,?4)",params![id,plan,status,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn operations(&self) -> Result<Vec<serde_json::Value>> {
        let mut s = self.conn.prepare(
            "SELECT status,payload FROM cleanup_operations ORDER BY rowid DESC LIMIT 200",
        )?;
        let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (status, payload) = r?;
            let mut value: serde_json::Value = serde_json::from_str(&payload)?;
            value["status"] = status.into();
            Ok(value)
        })
        .collect()
    }
    pub fn save_quarantine<T: serde::Serialize>(
        &self,
        id: &str,
        root: &str,
        state: &str,
        value: &T,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO quarantine_items(id,root,state,payload) VALUES(?1,?2,?3,?4)",
            params![id, root, state, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
    pub fn quarantine<T: serde::de::DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM quarantine_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn quarantine_items<T: serde::de::DeserializeOwned>(&self) -> Result<Vec<T>> {
        let mut s = self
            .conn
            .prepare("SELECT payload FROM quarantine_items ORDER BY rowid DESC LIMIT 1000")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn cache_entry(&self, key: &str) -> Result<Option<qqnt_model::CacheEntry>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM media_cache WHERE key=?1 AND payload IS NOT NULL",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    pub fn cache_entries(&self) -> Result<Vec<qqnt_model::CacheEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT payload FROM media_cache WHERE payload IS NOT NULL ORDER BY last_access ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn save_cache(&self, c: &qqnt_model::CacheEntry) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO media_cache(key,filename,size,created,last_access,digest,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![c.key,c.filename,c.size as i64,c.created,c.last_access,c.digest,serde_json::to_string(c)?])?;
        Ok(())
    }
    pub fn touch_cache(&self, key: &str) -> Result<()> {
        if let Some(mut entry) = self.cache_entry(key)? {
            entry.last_access = chrono::Utc::now().timestamp();
            self.save_cache(&entry)?;
        }
        Ok(())
    }
    pub fn remove_cache_entry(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM media_cache WHERE key=?1", [key])?;
        Ok(())
    }
}

impl Index {
    pub fn cached(
        &self,
        root: &str,
        path: &str,
        detection: &Detection,
        fp: &qqnt_model::Fingerprint,
    ) -> Result<Option<IndexedFile>> {
        let raw:Option<String>=self.conn.query_row("SELECT payload FROM scan_cache WHERE root=?1 AND path=?2 AND adapter=?3 AND fingerprint=?4",params![root,path,cache_tag(detection),serde_json::to_string(fp)?],|r|r.get(0)).optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Error::from))
            .transpose()
    }
    fn overlay_summary(&self, id: &str, root: &str, summary: &mut ScanSummary) -> Result<()> {
        let mut removed = self.conn.prepare("SELECT f.kind,f.risk,f.size,json_extract(f.payload,'$.directory'),f.modified FROM indexed_files f JOIN file_removals r ON r.path=f.path AND r.root=?1 AND r.fingerprint=json_extract(f.payload,'$.fingerprint') WHERE f.session_id=?2")?;
        let rows = removed.query_map(params![root, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?;
        for row in rows {
            let (kind, risk, size, directory, modified) = row?;
            summary.progress.files = summary.progress.files.saturating_sub(1);
            summary.progress.bytes = summary.progress.bytes.saturating_sub(size);
            let kind = match kind.as_str() {
                "Image" => qqnt_model::MediaKind::Image,
                "Video" => qqnt_model::MediaKind::Video,
                _ => qqnt_model::MediaKind::Other,
            };
            let risk = match risk.as_str() {
                "Protected" => qqnt_model::Risk::Protected,
                "ReviewRequired" => qqnt_model::Risk::ReviewRequired,
                "SafeCandidate" => qqnt_model::Risk::SafeCandidate,
                _ => qqnt_model::Risk::Unknown,
            };
            subtract_bucket(&mut summary.by_kind, &kind, size);
            subtract_bucket(&mut summary.by_risk, &risk, size);
            subtract_bucket(&mut summary.by_directory, &directory, size);
            let month = modified
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|t| t.format("%Y-%m").to_string())
                .unwrap_or_else(|| "未知时间".into());
            subtract_bucket(&mut summary.by_month, &month, size);
        }
        let any: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM protected_items WHERE root=?1)",
            [root],
            |r| r.get(0),
        )?;
        if !any {
            return Ok(());
        }
        let mut s=self.conn.prepare(&format!("SELECT CASE WHEN EXISTS(SELECT 1 FROM protected_items p WHERE p.root=?1 AND p.path=f.path) THEN 'Protected' ELSE f.risk END,count(*),sum(f.size) FROM indexed_files f JOIN scan_sessions s ON s.id=f.session_id WHERE f.session_id=?2 AND {PRESENT} GROUP BY 1"))?;
        let rows = s.query_map(params![root, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        summary.by_risk.clear();
        for row in rows {
            let (risk, count, bytes) = row?;
            let risk = match risk.as_str() {
                "Protected" => qqnt_model::Risk::Protected,
                "ReviewRequired" => qqnt_model::Risk::ReviewRequired,
                "SafeCandidate" => qqnt_model::Risk::SafeCandidate,
                _ => qqnt_model::Risk::Unknown,
            };
            summary.by_risk.insert(
                risk,
                qqnt_model::Bucket {
                    count: count as u64,
                    bytes: bytes as u64,
                },
            );
        }
        Ok(())
    }
    pub fn quarantine_page<T: serde::de::DeserializeOwned>(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<(u64, Vec<T>)> {
        let total: i64 = self
            .conn
            .query_row("SELECT count(*) FROM quarantine_items", [], |r| r.get(0))?;
        let mut s = self.conn.prepare(
            "SELECT payload FROM quarantine_items ORDER BY rowid DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = s.query_map(params![limit.min(200), offset], |r| r.get::<_, String>(0))?;
        let items = rows
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect::<Result<Vec<T>>>()?;
        Ok((total as u64, items))
    }
}

fn overlay(file: &mut IndexedFile, protected: bool) {
    if protected {
        file.user_protected = true;
        file.risk = qqnt_model::Risk::Protected;
        file.evidence.push("用户手动保护：拒绝清理优先".into());
    }
}

fn cache_tag(detection: &Detection) -> String {
    format!(
        "{}:{}:{}:classification-3",
        detection.adapter, detection.version, detection.recognized
    )
}

const PROTECTED: &str =
    "EXISTS(SELECT 1 FROM protected_items p WHERE p.root=s.root AND p.path=f.path)";
const PRESENT: &str = "NOT EXISTS(SELECT 1 FROM file_removals r WHERE r.root=s.root AND r.path=f.path AND r.fingerprint=json_extract(f.payload,'$.fingerprint'))";
fn file_conditions(query: &FileQuery) -> (String, Vec<Value>) {
    let protected = PROTECTED;
    let mut sql = format!(" WHERE f.session_id=? AND {PRESENT}");
    let mut values = vec![Value::Text(query.session_id.clone())];
    if let Some(kind) = query.kind {
        sql.push_str(" AND kind=?");
        values.push(Value::Text(format!("{kind:?}")));
    }
    if let Some(risk) = query.risk {
        sql.push_str(&format!(
            " AND (CASE WHEN {protected} THEN 'Protected' ELSE risk END)=?"
        ));
        values.push(Value::Text(format!("{risk:?}")));
    }
    if !query.search.is_empty() {
        sql.push_str(" AND instr(lower(path),lower(?))>0");
        values.push(Value::Text(query.search.clone()));
    }
    for (fragment, value) in [
        (
            " AND size>=?",
            query.min_size.map(|n| n.min(i64::MAX as u64) as i64),
        ),
        (
            " AND size<=?",
            query.max_size.map(|n| n.min(i64::MAX as u64) as i64),
        ),
        (" AND modified>=?", query.modified_from),
        (" AND modified<?", query.modified_to),
    ] {
        if let Some(n) = value {
            sql.push_str(fragment);
            values.push(Value::Integer(n));
        }
    }
    (sql, values)
}

fn subtract_bucket<K: Ord>(
    map: &mut std::collections::BTreeMap<K, qqnt_model::Bucket>,
    key: &K,
    size: u64,
) {
    if let Some(value) = map.get_mut(key) {
        value.count = value.count.saturating_sub(1);
        value.bytes = value.bytes.saturating_sub(size);
        if value.count == 0 && value.bytes == 0 {
            map.remove(key);
        }
    }
}
