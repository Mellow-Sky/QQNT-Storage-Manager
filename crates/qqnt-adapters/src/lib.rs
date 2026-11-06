use cap_std::fs::{Dir, Metadata};
use qqnt_model::Detection;
use std::{
    io,
    path::{Component, Path},
};

pub trait QQNTAdapter {
    fn detect(&self, root: &Dir) -> Detection;
    fn scan_locations(&self) -> Vec<&'static str>;
    fn protected_locations(&self) -> Vec<&'static str>;
}

pub struct StructuralAdapter;
impl QQNTAdapter for StructuralAdapter {
    fn detect(&self, root: &Dir) -> Detection {
        let present: Vec<_> = ["Pic", "Video", "Emoji", "File", "nt_db"]
            .into_iter()
            .filter(|name| {
                root.symlink_metadata(name)
                    .is_ok_and(|m| m.is_dir() && !is_link(&m))
            })
            .collect();
        let recognized = (present.contains(&"nt_db")
            && present.iter().filter(|x| **x != "nt_db").count() >= 2)
            || (present.contains(&"Pic") && present.contains(&"Emoji"));
        Detection {
            adapter: "qqnt-structural".into(),
            version: "1.1.0".into(),
            recognized,
            read_only: !recognized,
            evidence: vec![
                format!("检测到结构标记：{}", present.join(", ")),
                if present.contains(&"nt_db") {
                    "数据库标记位于授权根内，始终保护".into()
                } else {
                    "根内未发现 nt_db；数据库可能位于同级，不访问授权根之外".into()
                },
                if recognized {
                    "仅确认候选目录结构，未证明媒体可恢复或可删除".into()
                } else {
                    "布局未经验证，使用通用只读文件分析".into()
                },
            ],
        }
    }
    fn scan_locations(&self) -> Vec<&'static str> {
        vec!["."]
    }
    fn protected_locations(&self) -> Vec<&'static str> {
        vec![
            "nt_db",
            "config",
            "account",
            "File",
            "FileRecv",
            "favorites",
            "personal_emoji",
        ]
    }
}

pub fn is_link(meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|c| match c {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                !name.contains(':')
                    && !name.chars().any(char::is_control)
                    && !name.ends_with(['.', ' '])
            }
            _ => false,
        })
}

/// All component checks and opens remain relative to the retained root handle.
pub fn reject_links(root: &Dir, path: &Path) -> io::Result<()> {
    if !safe_relative(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid relative path",
        ));
    }
    let mut part = std::path::PathBuf::new();
    for c in path.components() {
        part.push(c);
        if is_link(&root.symlink_metadata(&part)?) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "link/reparse point refused",
            ));
        }
    }
    Ok(())
}

/// The selected directory itself must not be an alias. No drive-wide search.
pub fn open_authorized(path: &Path) -> io::Result<(std::path::PathBuf, Dir)> {
    for ancestor in path.ancestors() {
        let m = std::fs::symlink_metadata(ancestor)?;
        if m.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "linked root refused",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "reparse root refused",
                ));
            }
        }
    }
    let canonical = path.canonicalize()?;
    let dir = Dir::open_ambient_dir(&canonical, cap_std::ambient_authority())?;
    Ok((canonical, dir))
}
