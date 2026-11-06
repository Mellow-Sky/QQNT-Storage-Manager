use base64::{engine::general_purpose::STANDARD, Engine};
use cap_std::fs::Dir;
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use qqnt_adapters::reject_links;
use qqnt_model::{IndexedFile, MediaKind, MediaPreview};
use std::{
    fs::File,
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

pub const MAX_ENCODED_IMAGE: u64 = 64 * 1024 * 1024;
pub const MAX_VIDEO_CHUNK: u64 = 8 * 1024 * 1024;

pub fn open_indexed(root: &Dir, file: &IndexedFile) -> io::Result<File> {
    let path = Path::new(&file.relative_path);
    reject_links(root, path)?;
    let opened = root.open(path)?.into_std();
    let expected = file
        .fingerprint
        .as_ref()
        .ok_or_else(|| io::Error::other("此索引没有文件身份，请重新扫描"))?;
    if !qqnt_fs::unchanged(expected, &qqnt_fs::fingerprint(&opened)?) {
        return Err(io::Error::other("文件已变化，请重新扫描"));
    }
    Ok(opened)
}

pub fn image_preview(root: &Dir, file: &IndexedFile, full: bool) -> io::Result<MediaPreview> {
    if file.kind != MediaKind::Image {
        return Err(io::Error::other("不是支持的图片类型"));
    }
    let opened = open_indexed(root, file)?;
    if opened.metadata()?.len() > MAX_ENCODED_IMAGE {
        return Err(io::Error::other("图片超过 64 MiB 预览上限"));
    }
    let mut reader = ImageReader::new(BufReader::new(opened)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| io::Error::other("图片损坏、格式不支持或超过解码资源限制"))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image =
        image::DynamicImage::from_decoder(decoder).map_err(|_| io::Error::other("无法解码图片"))?;
    image.apply_orientation(orientation);
    let (original_width, original_height) = (image.width(), image.height());
    let edge = if full { 2048 } else { 256 };
    let output = image.thumbnail(edge, edge).to_rgb8();
    let (width, height) = output.dimensions();
    let mut buffer = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(output)
        .write_to(&mut buffer, ImageFormat::Jpeg)
        .map_err(|_| io::Error::other("预览编码失败"))?;
    Ok(MediaPreview {
        data_url: format!(
            "data:image/jpeg;base64,{}",
            STANDARD.encode(buffer.into_inner())
        ),
        width,
        height,
        original_width,
        original_height,
    })
}

pub struct VideoChunk {
    pub bytes: Vec<u8>,
    pub start: u64,
    pub end: u64,
    pub total: u64,
    pub mime: &'static str,
}

pub fn video_chunk(root: &Dir, file: &IndexedFile, range: Option<&str>) -> io::Result<VideoChunk> {
    if file.kind != MediaKind::Video {
        return Err(io::Error::other("不是视频"));
    }
    let mut opened = open_indexed(root, file)?;
    let total = opened.metadata()?.len();
    if total == 0 {
        return Err(io::Error::other("视频为空"));
    }
    let (start, end) = parse_range(range, total)?;
    let mut head = [0u8; 12];
    let n = opened.read(&mut head)?;
    let mime = if n >= 8 && &head[4..8] == b"ftyp" {
        "video/mp4"
    } else if head.starts_with(b"\x1a\x45\xdf\xa3") {
        "video/webm"
    } else if head.starts_with(b"RIFF") {
        "video/x-msvideo"
    } else {
        return Err(io::Error::other("视频头不支持，可能损坏或加密"));
    };
    opened.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0u8; (end - start + 1) as usize];
    opened.read_exact(&mut bytes)?;
    Ok(VideoChunk {
        bytes,
        start,
        end,
        total,
        mime,
    })
}

pub fn parse_range(range: Option<&str>, total: u64) -> io::Result<(u64, u64)> {
    if total == 0 {
        return Err(io::Error::other("empty media"));
    }
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid or unsatisfiable byte range",
        )
    };
    let Some(range) = range else {
        return Ok((0, (total - 1).min(MAX_VIDEO_CHUNK - 1)));
    };
    let range = range.strip_prefix("bytes=").ok_or_else(invalid)?;
    if range.contains(',') {
        return Err(invalid());
    }
    let (a, b) = range.split_once('-').ok_or_else(invalid)?;
    let (start, end) = if a.is_empty() {
        let n = b.parse::<u64>().map_err(|_| invalid())?;
        if n == 0 {
            return Err(invalid());
        }
        (total.saturating_sub(n), total - 1)
    } else {
        let start = a.parse::<u64>().map_err(|_| invalid())?;
        let end = if b.is_empty() {
            total - 1
        } else {
            b.parse::<u64>().map_err(|_| invalid())?
        };
        (start, end.min(total - 1))
    };
    if start >= total || start > end {
        return Err(invalid());
    }
    Ok((start, end.min(start.saturating_add(MAX_VIDEO_CHUNK - 1))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_are_bounded_and_reject_ambiguous_or_outside_requests() {
        assert_eq!(parse_range(Some("bytes=10-20"), 100).unwrap(), (10, 20));
        assert_eq!(parse_range(Some("bytes=-10"), 100).unwrap(), (90, 99));
        assert_eq!(
            parse_range(Some("bytes=0-"), u64::MAX).unwrap(),
            (0, MAX_VIDEO_CHUNK - 1)
        );
        for value in [
            "bytes=100-",
            "bytes=3-2",
            "bytes=0-1,4-5",
            "bytes=-0",
            "garbage",
        ] {
            assert!(parse_range(Some(value), 100).is_err());
        }
    }
    #[test]
    fn real_image_decoding_and_corruption_fallback() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("image.png");
        image::RgbImage::from_pixel(400, 200, image::Rgb([20, 100, 200]))
            .save(&path)
            .map_err(io::Error::other)?;
        let (.., root) = qqnt_adapters::open_authorized(temp.path())?;
        let file = File::open(&path)?;
        let mut record = IndexedFile {
            relative_path: "image.png".into(),
            directory: ".".into(),
            size: file.metadata()?.len(),
            created: None,
            modified: None,
            kind: MediaKind::Image,
            risk: qqnt_model::Risk::ReviewRequired,
            evidence: vec![],
            fingerprint: Some(qqnt_fs::fingerprint(&file)?),
            user_protected: false,
        };
        drop(file);
        let preview = image_preview(&root, &record, false)?;
        assert_eq!((preview.width, preview.height), (256, 128));
        assert!(preview.data_url.starts_with("data:image/jpeg;base64,"));
        std::fs::write(&path, b"bad")?;
        assert!(image_preview(&root, &record, false).is_err());
        record.fingerprint = Some(qqnt_fs::fingerprint(&File::open(&path)?)?);
        assert!(image_preview(&root, &record, false).is_err());
        Ok(())
    }
}
