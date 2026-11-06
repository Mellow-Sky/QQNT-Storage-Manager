//! Mutations operate on exclusively opened file handles while retaining every
//! ancestor directory handle without FILE_SHARE_WRITE or FILE_SHARE_DELETE.
use cap_std::fs::Dir;
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::*;

pub struct LockedFile {
    pub file: File,
    _parents: Vec<File>,
}
pub struct LockedDirectory {
    pub file: File,
    _parents: Vec<File>,
}

fn identity(file: &File) -> io::Result<String> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("reparse point refused"));
    }
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}

pub fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let n = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if n == 0 || n as usize >= buf.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(
        String::from_utf16(&buf[..n as usize])
            .map_err(|_| io::Error::other("unsupported path encoding"))?,
    ))
}
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_uppercase()
        == b.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_uppercase()
}
fn open_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("directory is not a regular directory"));
    }
    if !same_path(&final_path(&file)?, path) {
        return Err(io::Error::other("directory resolved to a different path"));
    }
    Ok(file)
}

fn root_guards(root: &Dir, root_path: &Path) -> io::Result<Vec<File>> {
    if !matches!(root_path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
    {
        return Err(io::Error::other("mutations require a local Windows disk"));
    }
    // A locked root alone does not prevent renaming one of its ancestors.
    // Hold the entire absolute chain so subsequent absolute handle operations
    // cannot resolve through a replacement ancestor or junction.
    let mut guards = Vec::new();
    let mut current = PathBuf::new();
    for component in root_path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::RootDir | Component::Normal(_)) {
            guards.push(open_directory(&current)?);
        }
    }
    let approved = root.try_clone()?.into_std_file();
    let guard = guards
        .last()
        .ok_or_else(|| io::Error::other("missing root"))?;
    if identity(&approved)? != identity(guard)? {
        return Err(io::Error::other("authorized root identity changed"));
    }
    Ok(guards)
}

fn parents(root: &Dir, root_path: &Path, relative: &Path) -> io::Result<Vec<File>> {
    if !qqnt_adapters::safe_relative(relative) {
        return Err(io::Error::other("unsafe relative path"));
    }
    let mut guards = root_guards(root, root_path)?;
    let mut current = root_path.to_path_buf();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            current.push(part);
            guards.push(open_directory(&current)?);
        }
    }
    Ok(guards)
}

pub fn lock_file(root: &Dir, root_path: &Path, relative: &Path) -> io::Result<LockedFile> {
    let guards = parents(root, root_path, relative)?;
    let absolute = root_path.join(relative);
    let file = OpenOptions::new()
        .access_mode(GENERIC_READ | DELETE | SYNCHRONIZE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(&absolute)?;
    if !same_path(&final_path(&file)?, &absolute) {
        return Err(io::Error::other("file resolved outside its indexed path"));
    }
    let fp = super::fingerprint(&file)?;
    if fp.links != 1 {
        return Err(io::Error::other("hardlinked files cannot be mutated"));
    }
    Ok(LockedFile {
        file,
        _parents: guards,
    })
}

pub fn lock_directory(
    root: &Dir,
    root_path: &Path,
    relative: &Path,
) -> io::Result<LockedDirectory> {
    let guards = parents(root, root_path, relative)?;
    let file = open_directory(&root_path.join(relative))?;
    Ok(LockedDirectory {
        file,
        _parents: guards,
    })
}

pub fn destination_parent(
    root: &Dir,
    root_path: &Path,
    relative: &Path,
    create_missing: bool,
) -> io::Result<LockedDirectory> {
    if !qqnt_adapters::safe_relative(relative) {
        return Err(io::Error::other("unsafe destination path"));
    }
    let mut guards = root_guards(root, root_path)?;
    let mut current = root_path.to_path_buf();
    if let Some(parent) = relative.parent() {
        for c in parent.components() {
            current.push(c);
            if create_missing {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            guards.push(open_directory(&current)?);
        }
    }
    let file = guards
        .pop()
        .ok_or_else(|| io::Error::other("missing root guard"))?;
    Ok(LockedDirectory {
        file,
        _parents: guards,
    })
}

impl LockedFile {
    /// Destination ancestors must also be retained by lock_directory/lock_file.
    pub fn rename_to(&mut self, destination: &Path) -> io::Result<()> {
        use std::os::windows::ffi::OsStrExt;
        let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
        let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + name.len() * 2;
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = std::ptr::null_mut();
            (*info).FileNameLength = (name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        }
        if unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                size as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub fn delete(&mut self) -> io::Result<()> {
        let mut info = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle(),
                FileDispositionInfo,
                (&mut info as *mut FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
