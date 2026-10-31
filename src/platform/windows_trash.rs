//! Handle-based, no-replacement same-volume moves. No Shell involvement.
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};
use std::ptr::null_mut;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

use super::{current_sid, pin_directory};
use crate::model::{FileTime, FormatVersion, Metadata, WindowsPath};
use crate::transaction::{self, Names, Operations};

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) || value.len() >= 32_768 {
        return Err(io::Error::other("NUL or overlong native path"));
    }
    value.push(0);
    Ok(value)
}

fn reject_components(path: &Path) -> io::Result<()> {
    for component in path.components() {
        if let Component::Normal(name) = component {
            let units: Vec<_> = name.encode_wide().collect();
            if units.contains(&(b':' as u16))
                || units.last().is_some_and(|u| [32, 46].contains(u))
                || name.to_string_lossy().eq_ignore_ascii_case("$Recycle.Bin")
            {
                return Err(io::Error::other(
                    "Recycle Bin paths, alternate streams, and trailing-dot/space components are unsupported",
                ));
            }
        }
    }
    Ok(())
}

fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: handle is live and buffer has the advertised capacity.
    let len = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if len >= buffer.len() || !buffer[..len].starts_with(&[92, 92, 63, 92]) {
        return Err(io::Error::other("unsupported resolved path"));
    }
    // A DOS drive path is needed for Explorer's original-path metadata.
    let units = &buffer[4..len];
    if units.len() < 3 || units[1..3] != [58, 92] {
        return Err(io::Error::other(
            "resolved source is not a local DOS drive path",
        ));
    }
    Ok(PathBuf::from(OsString::from_wide(units)))
}

/// Pin every ancestor, top-down, rejecting even intermediate reparse points.
fn pin_ancestors(path: &Path) -> io::Result<Vec<File>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("volume roots are unsupported"))?;
    let mut parents: Vec<_> = parent.ancestors().collect();
    parents.reverse();
    parents.into_iter().map(pin_directory).collect()
}

fn destination(source: &Path) -> io::Result<PathBuf> {
    let source = wide(source)?;
    let mut mount = vec![0u16; 32_768];
    let mut guid = [0u16; 1024];
    let mut filesystem = [0u16; 64];
    // SAFETY: all inputs are NUL terminated; output buffers match their lengths.
    unsafe {
        if GetVolumePathNameW(source.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) == 0 {
            return Err(io::Error::last_os_error());
        }
        if ![DRIVE_FIXED, DRIVE_REMOVABLE].contains(&GetDriveTypeW(mount.as_ptr())) {
            return Err(io::Error::other(
                "only local fixed/removable NTFS volumes are supported",
            ));
        }
        if GetVolumeNameForVolumeMountPointW(mount.as_ptr(), guid.as_mut_ptr(), guid.len() as u32)
            == 0
            || GetVolumeInformationW(
                guid.as_ptr(),
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    let len = filesystem
        .iter()
        .position(|&u| u == 0)
        .unwrap_or(filesystem.len());
    if !String::from_utf16_lossy(&filesystem[..len]).eq_ignore_ascii_case("NTFS") {
        return Err(io::Error::other("only NTFS volumes are supported"));
    }
    let len = guid
        .iter()
        .position(|&u| u == 0)
        .ok_or_else(|| io::Error::other("invalid volume GUID"))?;
    let id = super::volume_id(&guid[..len])?;
    let root = PathBuf::from(format!("\\\\?\\Volume{{{id}}}\\"));
    Ok(root.join("$Recycle.Bin").join(current_sid()?))
}

/// The zeroed union leaves ReplaceIfExists FALSE. Never use std::fs::rename,
/// whose replacement semantics differ between platforms.
fn rename(file: &File, destination: &Path) -> io::Result<()> {
    let name = wide(destination)?;
    let name_bytes = (name.len() - 1) * 2;
    let length = (offset_of!(FILE_RENAME_INFO, FileName) + name.len() * 2)
        .max(size_of::<FILE_RENAME_INFO>());
    // usize storage provides the alignment required by FILE_RENAME_INFO.
    let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: buffer is aligned, zero-initialized, and includes the variable tail.
    // Both the source and every destination ancestor remain pinned by the caller.
    unsafe {
        (*info).FileNameLength = name_bytes as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            length as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

struct Native {
    source: File,
    original: PathBuf,
    root: PathBuf,
    extension: OsString,
}

impl Operations for Native {
    type Stage = File;

    fn names(&mut self) -> io::Result<Names> {
        let mut random = [0u8; 6];
        // SAFETY: valid writable buffer; system-preferred RNG requires a NULL algorithm.
        let status = unsafe {
            BCryptGenRandom(
                null_mut(),
                random.as_mut_ptr(),
                random.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status < 0 {
            return Err(io::Error::other(format!("system RNG failed: {status:#x}")));
        }
        let alphabet = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let suffix: String = random
            .iter()
            .map(|b| alphabet[usize::from(*b) % alphabet.len()] as char)
            .collect();
        let mut name = OsString::from(format!("$I{suffix}"));
        name.push(&self.extension);
        let pair = crate::model::EntryNames::from_name(&name).map_err(io::Error::other)?;
        Ok(Names {
            metadata: self.root.join(pair.metadata),
            payload: self.root.join(pair.payload),
            stage: self.root.join(format!(".wtrash-{suffix}.tmp")),
        })
    }

    fn create(&mut self, names: &Names) -> io::Result<File> {
        OpenOptions::new()
            .access_mode(DELETE | GENERIC_READ | GENERIC_WRITE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .create_new(true)
            .open(&names.stage)
    }

    fn write(&mut self, stage: &mut File, bytes: &[u8]) -> io::Result<()> {
        stage.write_all(bytes)?;
        stage.sync_all()
    }

    fn move_payload(&mut self, names: &Names) -> io::Result<()> {
        // Avoid attaching our payload to already-present foreign metadata. This
        // precheck is only an optimization: both subsequent renames still refuse
        // replacement atomically, including collisions introduced after this check.
        match std::fs::symlink_metadata(&names.metadata) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "metadata name occupied",
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        rename(&self.source, &names.payload)
    }

    fn publish(&mut self, stage: &File, names: &Names) -> io::Result<()> {
        rename(stage, &names.metadata)
    }

    fn rollback(&mut self) -> io::Result<()> {
        rename(&self.source, &self.original)
    }

    fn cleanup(&mut self, stage: &File) -> io::Result<()> {
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: valid handle owned by us and correctly sized immutable input.
        if unsafe {
            SetFileInformationByHandle(
                stage.as_raw_handle(),
                FileDispositionInfo,
                (&info as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub(super) fn trash(path: &Path, dry_run: bool) -> io::Result<PathBuf> {
    let input = wide(path)?;
    // Reject UNC, extended/device namespaces, and root-relative ambiguity upfront.
    if input.starts_with(&[92, 92]) || input.starts_with(&[47, 47]) {
        return Err(io::Error::other(
            "network and device namespace paths are unsupported",
        ));
    }
    reject_components(path)?;
    let original = std::path::absolute(path)?;
    let units = wide(&original)?;
    if units.len() < 4 || units[1..3] != [58, 92] || original.file_name().is_none() {
        return Err(io::Error::other(
            "expected a file on a local drive; volume roots are unsupported",
        ));
    }
    reject_components(&original)?;
    let _source_pins = pin_ancestors(&original)?;
    let source = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&original)?;
    let attributes = source.metadata()?;
    if !attributes.is_file() || attributes.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "only ordinary files are supported; directories and reparse points are rejected",
        ));
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: valid source handle and writable output structure.
    if unsafe { GetFileInformationByHandle(source.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.nNumberOfLinks != 1 {
        return Err(io::Error::other(
            "multiply hard-linked files are unsupported",
        ));
    }
    // Resolve short aliases too, so an alias for $Recycle.Bin cannot bypass rejection.
    let original = final_path(&source)?;
    reject_components(&original)?;
    let _resolved_pins = pin_ancestors(&original)?;
    let root = destination(&original)?;
    let recycle = root
        .parent()
        .ok_or_else(|| io::Error::other("invalid recycle root"))?;
    let _recycle_pin = pin_directory(recycle).map_err(|e| io::Error::other(format!(
        "Recycle Bin parent unavailable ({e}); initialize it using Windows; wtrash will not create directories or change ACLs"
    )))?;
    let _user_pin = pin_directory(&root).map_err(|e| io::Error::other(format!(
        "current SID directory unavailable ({e}); initialize this volume's Recycle Bin using Windows first"
    )))?;
    let mut extension = OsString::new();
    if let Some(ext) = original.extension().filter(|e| !e.is_empty()) {
        extension.push(OsStr::new("."));
        extension.push(ext);
    }
    if extension.encode_wide().count() > 247 {
        return Err(io::Error::other(
            "extension is too long for a native $I/$R filename",
        ));
    }
    let original_path = WindowsPath::from_path(&original).map_err(io::Error::other)?;
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let ticks = u64::try_from(elapsed.as_nanos() / 100)
        .ok()
        .and_then(|v| v.checked_add(FileTime::UNIX_EPOCH_TICKS))
        .ok_or_else(|| io::Error::other("deletion timestamp out of range"))?;
    let metadata = Metadata {
        version: FormatVersion::V2,
        original_size: attributes.len(),
        deleted_at: FileTime(ticks),
        original_path,
    };
    let bytes = crate::codec::encode_v2(&metadata).map_err(io::Error::other)?;
    if dry_run {
        return Ok(root);
    }
    let mut native = Native {
        source,
        original,
        root,
        extension,
    };
    transaction::commit(&mut native, &bytes)
}

/// Open the exact object without following a final reparse point. Deny writes
/// and delete sharing so concurrent wtrash operations cannot change this selection.
fn open_entry(path: &Path, directory: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(DELETE | GENERIC_READ | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(io::Error::other(
            "entry type changed or reparse point encountered",
        ));
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: live handle and valid output storage.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !directory && info.nNumberOfLinks != 1 {
        return Err(io::Error::other(
            "multiply hard-linked entries are unsupported",
        ));
    }
    Ok(file)
}

fn dispose(file: &File) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: live DELETE handle and a correctly sized structure.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Existing ancestors are pinned top-down. A newly created directory is pinned
/// and checked before it is used; a concurrently inserted junction is rejected.
fn restore_parents(path: &Path, create: bool, dry_run: bool) -> io::Result<Vec<File>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    let mut ancestors: Vec<_> = parent.ancestors().collect();
    ancestors.reverse();
    let mut pins = Vec::new();
    for ancestor in ancestors {
        match pin_directory(ancestor) {
            Ok(pin) => {
                reject_components(&final_path(&pin)?)?;
                pins.push(pin);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                if dry_run {
                    // No descendants can exist below a missing component.
                    break;
                }
                match std::fs::create_dir(ancestor) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
                let pin = pin_directory(ancestor)?;
                reject_components(&final_path(&pin)?)?;
                pins.push(pin);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(pins)
}

struct DeleteNode {
    file: File,
    children: Vec<DeleteNode>,
}

/// Preflight the entire tree without following links. Retain every handle so a
/// validated name cannot be replaced. Bound depth and handles before deleting.
fn deletion_tree(
    file: File,
    path: &Path,
    depth: usize,
    budget: &mut usize,
) -> io::Result<DeleteNode> {
    if depth > 128 || *budget == 0 {
        return Err(io::Error::other(
            "directory deletion safety limit exceeded (128 levels / 16384 objects)",
        ));
    }
    *budget -= 1;
    let mut children = Vec::new();
    if file.metadata()?.is_dir() {
        for child in std::fs::read_dir(path)? {
            let child = child?;
            let path = child.path();
            let attributes = std::fs::symlink_metadata(&path)?;
            // open_entry checks attributes again using the actual handle.
            let file = open_entry(&path, attributes.is_dir())?;
            children.push(deletion_tree(file, &path, depth + 1, budget)?);
        }
    }
    Ok(DeleteNode { file, children })
}

fn delete_tree(node: DeleteNode) -> io::Result<()> {
    for child in node.children {
        delete_tree(child)?;
    }
    dispose(&node.file)
}

pub(super) fn mutate(
    entry: &crate::listing::Entry,
    restore: Option<bool>,
    dry_run: bool,
) -> io::Result<()> {
    use std::io::Read;
    let root = PathBuf::from(format!("\\\\?\\Volume{{{}}}\\", entry.volume))
        .join("$Recycle.Bin")
        .join(current_sid()?);
    // Roots come from a freshly enumerated current-user entry. Re-pin and open
    // the exact names, then re-decode and compare metadata after obtaining locks.
    let _parent = pin_directory(
        root.parent()
            .ok_or_else(|| io::Error::other("invalid root"))?,
    )?;
    let _user = pin_directory(&root)?;
    let name = OsString::from_wide(entry.id.filename_utf16());
    let pair = crate::model::EntryNames::from_name(&name).map_err(io::Error::other)?;
    let metadata_path = root.join(pair.metadata);
    let payload_path = root.join(pair.payload);
    let metadata_file = open_entry(&metadata_path, false)?;
    let mut bytes = Vec::new();
    (&metadata_file)
        .take((crate::codec::MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let metadata = crate::codec::decode(&bytes).map_err(io::Error::other)?;
    if metadata != entry.metadata {
        return Err(io::Error::other(
            "entry metadata changed since selection; list again",
        ));
    }
    crate::restore::validate_destination(metadata.original_path.as_utf16())?;
    let directory = entry.kind == crate::listing::EntryKind::Directory;
    let payload = open_entry(&payload_path, directory)?;
    if let Some(create) = restore {
        let original = metadata
            .original_path
            .to_path_buf()
            .map_err(io::Error::other)?;
        // Pin the drive root and check volume identity BEFORE creating parents.
        let drive = original
            .ancestors()
            .last()
            .ok_or_else(|| io::Error::other("missing drive"))?;
        let _drive = pin_directory(drive)?;
        if destination(drive)? != root {
            return Err(io::Error::other(
                "original drive now refers to a different volume; refusing cross-volume restore",
            ));
        }
        let _pins = restore_parents(&original, create, dry_run)?;
        match std::fs::symlink_metadata(&original) {
            Ok(_) => return Err(io::Error::other("restore destination already exists")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if dry_run {
            return Ok(());
        }
        rename(&payload, &original)?;
        // Never roll back or delete restored data if metadata cleanup fails.
        dispose(&metadata_file).map_err(|e| io::Error::other(format!(
            "payload restored to {original:?}, but metadata cleanup failed at {metadata_path:?}: {e}; do not retry blindly"
        )))?;
    } else {
        let tree = deletion_tree(payload, &payload_path, 0, &mut 16_384)?;
        if dry_run {
            return Ok(());
        }
        delete_tree(tree).map_err(|e| io::Error::other(format!(
            "payload deletion failed and may be partial: {e}; metadata retained at {metadata_path:?}; inspect remaining data before retrying"
        )))?;
        dispose(&metadata_file).map_err(|e| {
            io::Error::other(format!(
                "payload deleted, but metadata cleanup failed at {metadata_path:?}: {e}"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod mutation_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "wtrash-mutation-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // All native fixtures use private temporary directories, never a real bin.
    #[test]
    fn move_collision_preserves_both_objects_and_retry_succeeds() {
        let fixture = Fixture::new();
        let payload = fixture.0.join("日本語 payload");
        let target = fixture.0.join("noextension");
        std::fs::write(&payload, b"recoverable").unwrap();
        std::fs::write(&target, b"existing").unwrap();
        let file = open_entry(&payload, false).unwrap();
        assert!(rename(&file, &target).is_err());
        assert_eq!(std::fs::read(&payload).unwrap(), b"recoverable");
        assert_eq!(std::fs::read(&target).unwrap(), b"existing");
        std::fs::remove_file(&target).unwrap();
        rename(&file, &target).unwrap();
        drop(file);
        assert_eq!(std::fs::read(&target).unwrap(), b"recoverable");
        assert!(!payload.exists());
    }

    #[test]
    fn missing_parents_and_dry_run() {
        let fixture = Fixture::new();
        let target = fixture.0.join("missing").join("nested").join("file");
        assert!(restore_parents(&target, false, false).is_err());
        assert!(restore_parents(&target, true, true).is_ok());
        assert!(!fixture.0.join("missing").exists());
        let pins = restore_parents(&target, true, false).unwrap();
        assert!(target.parent().unwrap().is_dir());
        drop(pins);
    }

    #[test]
    fn deletion_preflight_and_partial_failure_keep_metadata() {
        let fixture = Fixture::new();
        let payload = fixture.0.join("$Rtest");
        let metadata = fixture.0.join("$Itest");
        std::fs::create_dir(&payload).unwrap();
        std::fs::write(payload.join("child"), b"content").unwrap();
        std::fs::write(&metadata, b"record").unwrap();
        let tree =
            deletion_tree(open_entry(&payload, true).unwrap(), &payload, 0, &mut 100).unwrap();
        assert!(payload.join("child").exists()); // preflight changes nothing
        drop(tree);
        // Inject failure with a handle lacking DELETE access.
        let tree = DeleteNode {
            file: File::open(payload.join("child")).unwrap(),
            children: Vec::new(),
        };
        assert!(delete_tree(tree).is_err());
        assert_eq!(std::fs::read(&metadata).unwrap(), b"record");
        let tree =
            deletion_tree(open_entry(&payload, true).unwrap(), &payload, 0, &mut 100).unwrap();
        delete_tree(tree).unwrap();
        assert!(!payload.exists());
        assert!(metadata.exists());
    }

    #[test]
    fn concurrent_selection_and_preflight_limit_fail_safely() {
        let fixture = Fixture::new();
        let payload = fixture.0.join("payload");
        std::fs::write(&payload, b"content").unwrap();
        let file = open_entry(&payload, false).unwrap();
        assert!(open_entry(&payload, false).is_err());
        assert!(deletion_tree(file, &payload, 0, &mut 0).is_err());
        assert_eq!(std::fs::read(&payload).unwrap(), b"content");
    }
}
