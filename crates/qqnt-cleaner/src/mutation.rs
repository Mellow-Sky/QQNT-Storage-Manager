use super::*;
use qqnt_fs::windows::{destination_parent, lock_file, LockedFile};

fn vault_guard(root: &Dir, path: &Path, owner: &str) -> Result<LockedFile, String> {
    let mut guard =
        lock_file(root, path, &Path::new(VAULT).join("owner")).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    guard
        .file
        .by_ref()
        .take(128)
        .read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    if contents != owner {
        return Err("隔离区所有权标记已变化".into());
    }
    Ok(guard)
}

fn stopped(check: &impl Fn() -> io::Result<bool>) -> Result<(), String> {
    match check() {
        Ok(false) => Ok(()),
        Ok(true) => Err("QQNT 尚未完全退出，拒绝修改文件".into()),
        Err(_) => Err("无法可靠检查 QQNT 进程，拒绝修改文件".into()),
    }
}
fn stored(item: &QuarantineItem) -> Result<std::path::PathBuf, String> {
    if Uuid::parse_str(&item.id).is_err() || item.stored_name != format!("{}.bin", item.id) {
        return Err("隔离记录路径无效".into());
    }
    if !qqnt_adapters::safe_relative(Path::new(&item.relative_path))
        || qqnt_classifier::protected_path(Path::new(&item.relative_path))
    {
        return Err("隔离记录指向关键路径，拒绝操作".into());
    }
    Ok(Path::new(VAULT).join(&item.stored_name))
}
fn stored_media(file: &mut std::fs::File, item: &QuarantineItem) -> Result<(), String> {
    let mut head = [0u8; 32];
    file.rewind().map_err(|e| e.to_string())?;
    let n = file.read(&mut head).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    let (_, risk, _) = qqnt_classifier::classify(Path::new(&item.relative_path), &head[..n], true);
    if matches!(risk, Risk::Protected | Risk::Unknown) {
        return Err("隔离内容重新分类为受保护或未知数据，拒绝写操作".into());
    }
    Ok(())
}
fn commit_manifest(root: &Dir, item: &mut QuarantineItem) {
    if write_manifest(root, item, false).is_err() {
        item.error = Some("文件操作已完成，但隔离清单未能更新；请保留应用索引".into());
    }
}

#[allow(clippy::too_many_arguments)]
pub fn quarantine(
    root: &Dir,
    root_path: &Path,
    owner: &str,
    entry: &PlanEntry,
    mode: CleanupMode,
    review: bool,
    recognized: bool,
    item: &mut QuarantineItem,
    process_check: impl Fn() -> io::Result<bool>,
) -> Result<(), String> {
    stopped(&process_check)?;
    eligible(&entry.file, recognized, mode, review)?;
    if !entry.allowed {
        return Err("计划中的此项已被拒绝".into());
    }
    validate_vault(root, owner, true).map_err(|e| e.to_string())?;
    let destination = stored(item)?;
    let _vault = vault_guard(root, root_path, owner)?;
    let mut file = lock_file(root, root_path, Path::new(&entry.file.relative_path))
        .map_err(|e| e.to_string())?;
    let actual = qqnt_fs::fingerprint(&file.file).map_err(|e| e.to_string())?;
    if file
        .file
        .metadata()
        .map_err(|e| e.to_string())?
        .permissions()
        .readonly()
    {
        return Err("文件带有只读保护属性".into());
    }
    if entry.file.fingerprint.as_ref() != Some(&actual) {
        return Err("文件身份、大小或时间与扫描时不同".into());
    }
    let mut header = [0u8; 32];
    let n = file.file.read(&mut header).map_err(|e| e.to_string())?;
    file.file.rewind().map_err(|e| e.to_string())?;
    let (_, risk, _) = qqnt_classifier::classify(
        Path::new(&entry.file.relative_path),
        &header[..n],
        recognized,
    );
    let mut fresh = entry.file.clone();
    fresh.risk = risk;
    eligible(&fresh, recognized, mode, review)?;
    let digest = qqnt_fs::content_hash(&mut file.file).map_err(|e| e.to_string())?;
    if entry.digest.as_deref() != Some(&digest) || item.digest != digest {
        return Err("内容摘要已变化，计划失效".into());
    }
    write_manifest(root, item, true).map_err(|e| e.to_string())?;
    stopped(&process_check)?;
    file.rename_to(&root_path.join(destination))
        .map_err(|e| e.to_string())?;
    item.fingerprint = qqnt_fs::fingerprint(&file.file).map_err(|e| e.to_string())?;
    item.state = "quarantined".into();
    item.error = None;
    commit_manifest(root, item);
    Ok(())
}

pub fn restore(
    root: &Dir,
    root_path: &Path,
    owner: &str,
    item: &mut QuarantineItem,
    process_check: impl Fn() -> io::Result<bool>,
) -> Result<(), String> {
    stopped(&process_check)?;
    if item.state != "quarantined" {
        return Err("此文件不在可恢复状态".into());
    }
    validate_vault(root, owner, false).map_err(|e| e.to_string())?;
    let _vault = vault_guard(root, root_path, owner)?;
    let source = stored(item)?;
    let destination = Path::new(&item.relative_path);
    let _parent =
        destination_parent(root, root_path, destination, true).map_err(|e| e.to_string())?;
    let mut file = lock_file(root, root_path, &source).map_err(|e| e.to_string())?;
    if qqnt_fs::fingerprint(&file.file).map_err(|e| e.to_string())? != item.fingerprint {
        return Err("隔离文件身份已变化".into());
    }
    if qqnt_fs::content_hash(&mut file.file).map_err(|e| e.to_string())? != item.digest {
        return Err("隔离文件内容已变化".into());
    }
    stored_media(&mut file.file, item)?;
    stopped(&process_check)?;
    // ReplaceIfExists=false is enforced by the handle API, including at commit time.
    file.rename_to(&root_path.join(destination))
        .map_err(|e| e.to_string())?;
    item.state = "restored".into();
    item.error = None;
    commit_manifest(root, item);
    Ok(())
}

pub fn purge(
    root: &Dir,
    root_path: &Path,
    owner: &str,
    item: &mut QuarantineItem,
    process_check: impl Fn() -> io::Result<bool>,
) -> Result<(), String> {
    stopped(&process_check)?;
    if item.state != "quarantined" {
        return Err("仅允许删除当前隔离中的文件".into());
    }
    validate_vault(root, owner, false).map_err(|e| e.to_string())?;
    let _vault = vault_guard(root, root_path, owner)?;
    let source = stored(item)?;
    let mut file = lock_file(root, root_path, &source).map_err(|e| e.to_string())?;
    if qqnt_fs::fingerprint(&file.file).map_err(|e| e.to_string())? != item.fingerprint {
        return Err("隔离文件身份已变化".into());
    }
    if qqnt_fs::content_hash(&mut file.file).map_err(|e| e.to_string())? != item.digest {
        return Err("隔离文件内容已变化".into());
    }
    stored_media(&mut file.file, item)?;
    stopped(&process_check)?;
    file.delete().map_err(|e| e.to_string())?;
    drop(file);
    item.state = "deleted".into();
    item.error = None;
    commit_manifest(root, item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Result<
        (
            tempfile::TempDir,
            std::path::PathBuf,
            Dir,
            PlanEntry,
            CleanupPlan,
        ),
        Box<dyn std::error::Error>,
    > {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?;
        std::fs::create_dir(path.join("Pic"))?;
        std::fs::write(path.join("Pic/test.jpg"), b"\xff\xd8\xffmock image")?;
        let (_, root) = qqnt_adapters::open_authorized(&path)?;
        let opened = root.open("Pic/test.jpg")?.into_std();
        let file = IndexedFile {
            relative_path: "Pic/test.jpg".into(),
            directory: "Pic".into(),
            size: opened.metadata()?.len(),
            created: None,
            modified: None,
            kind: qqnt_model::MediaKind::Image,
            risk: Risk::ReviewRequired,
            evidence: vec![],
            fingerprint: Some(qqnt_fs::fingerprint(&opened)?),
            user_protected: false,
        };
        drop(opened);
        let plan = dry_run(
            &root,
            "s",
            path.to_str().ok_or("path encoding")?,
            "1",
            true,
            CleanupMode::Standard,
            true,
            vec![file],
        );
        assert_eq!(plan.files, 1);
        let entry = plan.entries[0].clone();
        Ok((temp, path, root, entry, plan))
    }
    #[test]
    fn quarantine_restore_no_overwrite_and_permanent_delete(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, path, root, entry, plan) = fixture()?;
        let owner = Uuid::new_v4().to_string();
        let mut item = pending_item(&plan, &entry)?;
        quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false),
        )?;
        assert!(!path.join("Pic/test.jpg").exists());
        assert!(path.join(VAULT).join(&item.stored_name).exists());
        std::fs::write(path.join("Pic/test.jpg"), b"newly received file")?;
        assert!(restore(&root, &path, &owner, &mut item, || Ok(false)).is_err());
        assert_eq!(
            std::fs::read(path.join("Pic/test.jpg"))?,
            b"newly received file"
        );
        std::fs::remove_file(path.join("Pic/test.jpg"))?;
        restore(&root, &path, &owner, &mut item, || Ok(false))?;
        assert_eq!(
            std::fs::read(path.join("Pic/test.jpg"))?,
            b"\xff\xd8\xffmock image"
        );
        let mut entry = entry;
        entry.file.fingerprint = Some(qqnt_fs::fingerprint(
            &root.open("Pic/test.jpg")?.into_std(),
        )?);
        let mut item = pending_item(&plan, &entry)?;
        quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false),
        )?;
        purge(&root, &path, &owner, &mut item, || Ok(false))?;
        assert!(!path.join(VAULT).join(&item.stored_name).exists());
        Ok(())
    }
    #[test]
    fn qq_running_changes_hardlinks_and_unknown_modes_are_rejected(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, path, root, entry, plan) = fixture()?;
        let owner = Uuid::new_v4().to_string();
        let mut item = pending_item(&plan, &entry)?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(true)
        )
        .is_err());
        std::fs::write(path.join("Pic/test.jpg"), b"changed")?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false)
        )
        .is_err());
        assert!(path.join("Pic/test.jpg").exists());
        std::fs::hard_link(path.join("Pic/test.jpg"), path.join("Pic/link.jpg"))?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false)
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn occupied_readonly_and_late_qq_start_leave_source_intact(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_temp, path, root, entry, plan) = fixture()?;
        let owner = Uuid::new_v4().to_string();
        let mut item = pending_item(&plan, &entry)?;
        let occupied = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path.join("Pic/test.jpg"))?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false)
        )
        .is_err());
        drop(occupied);
        let mut permissions = std::fs::metadata(path.join("Pic/test.jpg"))?.permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(path.join("Pic/test.jpg"), permissions)?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false)
        )
        .is_err());
        let mut permissions = std::fs::metadata(path.join("Pic/test.jpg"))?.permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path.join("Pic/test.jpg"), permissions)?;
        let mut entry = entry;
        entry.file.fingerprint = Some(qqnt_fs::fingerprint(
            &root.open("Pic/test.jpg")?.into_std(),
        )?);
        let calls = AtomicUsize::new(0);
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(calls.fetch_add(1, Ordering::SeqCst) > 0)
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(path.join("Pic/test.jpg").exists());
        Ok(())
    }

    #[test]
    fn partial_batch_and_junction_restore_are_safe() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, path, root, entry, plan) = fixture()?;
        let owner = Uuid::new_v4().to_string();
        let mut failed = entry.clone();
        failed.file.relative_path = "Pic/missing.jpg".into();
        let mut failed_item = pending_item(&plan, &failed)?;
        assert!(quarantine(
            &root,
            &path,
            &owner,
            &failed,
            plan.mode,
            true,
            true,
            &mut failed_item,
            || Ok(false)
        )
        .is_err());
        let mut item = pending_item(&plan, &entry)?;
        quarantine(
            &root,
            &path,
            &owner,
            &entry,
            plan.mode,
            true,
            true,
            &mut item,
            || Ok(false),
        )?;
        assert_eq!(item.state, "quarantined");
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(
            path.join(VAULT).join(format!("{}.json", item.id)),
        )?)?;
        assert_eq!(manifest["item"]["state"], "quarantined");
        std::fs::remove_dir(path.join("Pic"))?;
        let outside = tempfile::tempdir()?;
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(path.join("Pic"))
            .arg(outside.path())
            .output()?;
        assert!(made.status.success());
        let result = restore(&root, &path, &owner, &mut item, || Ok(false));
        std::fs::remove_dir(path.join("Pic"))?;
        assert!(result.is_err());
        assert!(!outside.path().join("test.jpg").exists());
        assert!(path.join(VAULT).join(&item.stored_name).exists());
        restore(&root, &path, &owner, &mut item, || Ok(false))?;
        assert!(path.join("Pic/test.jpg").exists());
        Ok(())
    }
}
