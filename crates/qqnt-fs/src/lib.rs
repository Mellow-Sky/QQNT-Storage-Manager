use qqnt_model::Fingerprint;
use std::{
    fs::File,
    io::{self, Read, Seek},
    time::SystemTime,
};
#[cfg(windows)]
pub mod windows;

pub fn nanos(time: io::Result<SystemTime>) -> Option<String> {
    time.ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos().to_string())
}

pub fn fingerprint(file: &File) -> io::Result<Fingerprint> {
    let m = file.metadata()?;
    if !m.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    #[cfg(unix)]
    let (identity, links, change_time) = {
        use std::os::unix::fs::MetadataExt;
        (
            format!("{}:{}", m.dev(), m.ino()),
            m.nlink(),
            Some(format!("{}:{}", m.ctime(), m.ctime_nsec())),
        )
    };
    #[cfg(windows)]
    let (identity, links, change_time) = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileBasicInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
            BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.dwFileAttributes & 0x400 != 0 {
            return Err(io::Error::other("reparse point refused"));
        }
        let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        (
            format!(
                "{:08x}:{:08x}{:08x}",
                info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
            ),
            info.nNumberOfLinks as u64,
            Some(basic.ChangeTime.to_string()),
        )
    };
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::other(
        "file identity is unsupported on this platform",
    ));
    #[cfg(any(unix, windows))]
    Ok(Fingerprint {
        identity,
        size: m.len(),
        modified_nanos: nanos(m.modified()),
        created_nanos: nanos(m.created()),
        change_time,
        links,
    })
}

pub fn content_hash(file: &mut File) -> io::Result<String> {
    file.rewind()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    file.rewind()?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn unchanged(expected: &Fingerprint, actual: &Fingerprint) -> bool {
    expected == actual
}

pub fn qq_is_running() -> io::Result<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
        };
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
        let mut running = false;
        while ok != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
            if ["qq.exe", "qqnt.exe", "qqguild.exe", "qqprotect.exe"].contains(&name.as_str()) {
                running = true;
                break;
            }
            ok = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        let error = unsafe { GetLastError() };
        unsafe { CloseHandle(snapshot) };
        if !running && error != ERROR_NO_MORE_FILES {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(running)
    }
    #[cfg(not(windows))]
    Err(io::Error::other("QQ process safety checks require Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_and_content_changes_are_detected() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let p = temp.path().join("file");
        std::fs::write(&p, b"original")?;
        let mut file = File::open(&p)?;
        let before = fingerprint(&file)?;
        let hash = content_hash(&mut file)?;
        assert_eq!(before.size, 8);
        assert!(!hash.is_empty());
        drop(file);
        std::fs::write(&p, b"changed content")?;
        assert!(!unchanged(&before, &fingerprint(&File::open(&p)?)?));
        Ok(())
    }
}
