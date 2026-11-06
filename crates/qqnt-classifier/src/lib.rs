use qqnt_model::{MediaKind, Risk};
use std::path::Path;

pub fn protected_path(path: &Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    parts.iter().any(|p| {
        [
            "nt_db",
            "db",
            "database",
            "config",
            "configs",
            "account",
            "accounts",
            "file",
            "filerecv",
            "favorites",
            "favourites",
            "personal_emoji",
            "收藏",
        ]
        .contains(&p.as_str())
    }) || [
        "db",
        "sqlite",
        "sqlite3",
        "db-wal",
        "db-shm",
        "db-journal",
        "wal",
        "shm",
        "journal",
        "ini",
        "json",
        "key",
        "pem",
        "conf",
    ]
    .contains(&ext.as_str())
        || parts.last().is_some_and(|n| {
            n.ends_with("-wal")
                || n.ends_with("-shm")
                || n.ends_with("-journal")
                || n.contains("account")
                || n.contains("config")
        })
}

pub fn classify(path: &Path, header: &[u8], recognized: bool) -> (MediaKind, Risk, Vec<String>) {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let signature = if header.starts_with(b"\x89PNG\r\n\x1a\n")
        || header.starts_with(b"\xff\xd8\xff")
        || header.starts_with(b"GIF87a")
        || header.starts_with(b"GIF89a")
        || header.starts_with(b"BM")
        || (header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP"))
    {
        Some(MediaKind::Image)
    } else if (header.get(4..8) == Some(b"ftyp")
        && !matches!(
            header.get(8..12),
            Some(b"avif" | b"avis" | b"heic" | b"heix" | b"mif1")
        ))
        || (header.starts_with(b"RIFF") && header.get(8..12) == Some(b"AVI "))
        || header.starts_with(b"\x1a\x45\xdf\xa3")
    {
        Some(MediaKind::Video)
    } else {
        None
    };
    let by_ext = if [
        "jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "heic", "tif", "tiff",
    ]
    .contains(&ext.as_str())
    {
        MediaKind::Image
    } else if ["mp4", "mov", "webm", "mkv", "avi", "m4v"].contains(&ext.as_str()) {
        MediaKind::Video
    } else {
        MediaKind::Other
    };
    let kind = signature.unwrap_or(by_ext);
    let mut evidence = vec![
        format!("扩展名：{}", if ext.is_empty() { "无" } else { &ext }),
        if signature.is_some() {
            "只读文件头匹配媒体特征（不代表可完整解码）".into()
        } else {
            "未匹配媒体文件头；类型仅供统计，可能损坏或为加密格式".into()
        },
    ];
    let database_header = header.starts_with(b"SQLite format 3\0");
    let risk = if protected_path(path) || database_header {
        evidence.push("拒绝优先：数据库、配置、收藏或接收文件保护规则".into());
        Risk::Protected
    } else if !recognized {
        evidence.push("未知布局：仅允许只读分析".into());
        Risk::Unknown
    } else if signature.is_some() {
        evidence.push("媒体可能被聊天记录引用，未证明可恢复，必须人工审核".into());
        Risk::ReviewRequired
    } else {
        evidence.push("没有足够证据证明是可清理缓存".into());
        Risk::Unknown
    };
    // No current rule can return SafeCandidate: directory names never establish recoverability.
    (kind, risk, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deny_wins_even_if_media_header() {
        for path in [
            "nt_db/test.png",
            "Pic/message.db-wal",
            "File/photo.jpg",
            "config/data.gif",
            "favorites/a.png",
            "Emoji/personal_emoji/Ori/a.png",
        ] {
            assert_eq!(
                classify(Path::new(path), b"\xff\xd8\xff", true).1,
                Risk::Protected
            );
        }
    }
    #[test]
    fn unknown_and_corrupt_are_never_safe() {
        assert_eq!(
            classify(Path::new("Pic/a.jpg"), b"broken", true).1,
            Risk::Unknown
        );
        assert_eq!(
            classify(Path::new("Pic/a.jpg"), b"\xff\xd8\xff", false).1,
            Risk::Unknown
        );
        assert_eq!(
            classify(Path::new("Pic/thumbnail.jpg"), b"\xff\xd8\xff", true).1,
            Risk::ReviewRequired
        );
        assert_eq!(
            classify(Path::new("Pic/misnamed.png"), b"SQLite format 3\0", true).1,
            Risk::Protected
        );
    }
}
