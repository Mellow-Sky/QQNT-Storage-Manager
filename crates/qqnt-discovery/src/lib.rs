use cap_std::fs::Dir;
use qqnt_adapters::{is_link, open_authorized, reject_links, QQNTAdapter, StructuralAdapter};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Only the current user's known directories; other profiles require manual selection.
pub fn default_containers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(doc) = dirs::document_dir() {
        out.push(doc.join("Tencent Files"));
    }
    if let Some(data) = dirs::data_local_dir() {
        out.push(data.join("Tencent/QQ/UserData"));
    }
    out
}

/// Inspect only explicit layout candidates and immediate account directories.
pub fn candidates(container: &Path) -> io::Result<Vec<PathBuf>> {
    let (base, dir) = open_authorized(container)?;
    if StructuralAdapter.detect(&dir).recognized {
        return Ok(vec![base]);
    }
    let mut found = Vec::new();
    inspect(&dir, &base, Path::new("nt_qq/nt_data"), &mut found);
    inspect(&dir, &base, Path::new("nt_data"), &mut found);
    for entry in dir.entries()?.take(4096).flatten() {
        let name = PathBuf::from(entry.file_name());
        let Ok(m) = dir.symlink_metadata(&name) else {
            continue;
        };
        if !m.is_dir() || is_link(&m) {
            continue;
        }
        inspect(&dir, &base, &name.join("nt_qq/nt_data"), &mut found);
        inspect(&dir, &base, &name.join("nt_data"), &mut found);
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn inspect(dir: &Dir, base: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    if reject_links(dir, path).is_err() {
        return;
    }
    if let Ok(child) = dir.open_dir(path) {
        if StructuralAdapter.detect(&child).recognized {
            out.push(base.join(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounts_unicode_custom_layout_and_empty() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(candidates(temp.path())?.is_empty());
        for account in ["中文账号", "second"] {
            for child in ["Pic", "Video", "nt_db"] {
                std::fs::create_dir_all(
                    temp.path().join(account).join("nt_qq/nt_data").join(child),
                )?;
            }
        }
        assert_eq!(candidates(temp.path())?.len(), 2);
        assert_eq!(candidates(&temp.path().join("中文账号"))?.len(), 1);
        Ok(())
    }

    #[test]
    fn sibling_database_layout_is_discovered_without_entering_database() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let account = temp.path().join("12345/nt_qq");
        for path in ["nt_data/Pic", "nt_data/Emoji", "nt_db"] {
            std::fs::create_dir_all(account.join(path))?;
        }
        let found = candidates(temp.path())?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], account.join("nt_data").canonicalize()?);
        assert_eq!(candidates(&account)?.len(), 1);
        Ok(())
    }
}
