//! Ordinary Win32 volume/token APIs only. No Shell APIs or external tools.
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::Storage::FileSystem::{
    FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDriveTypeW, GetVolumeInformationW,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

use super::BackendError;
use crate::listing::{self, Listing, RecycleRoot, VolumeId};

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns a successful OpenProcessToken handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct VolumeSearch(HANDLE);
impl Drop for VolumeSearch {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns a successful FindFirstVolume handle.
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}

fn native(operation: &'static str, source: io::Error) -> BackendError {
    BackendError::Native { operation, source }
}

fn current_sid() -> io::Result<OsString> {
    // SAFETY: all API output pointers refer to live, appropriately sized storage;
    // token information uses usize-aligned storage and remains alive during conversion.
    unsafe {
        let mut handle = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Token(handle);
        let mut required = 0;
        let status = GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        if status != 0
            || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        {
            return Err(io::Error::other(
                "unexpected token information sizing result",
            ));
        }
        if required < size_of::<TOKEN_USER>() as u32 || required > 65_536 {
            return Err(io::Error::other("invalid token information size"));
        }
        let mut buffer = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        let length = size_of_val(buffer.as_slice()) as u32;
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut required,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        // SID text from Windows is ASCII, bounded by the maximum SID size.
        let result = (|| {
            let mut len = 0;
            while len < 256 && *sid.add(len) != 0 {
                len += 1;
            }
            if len == 256 {
                return Err(io::Error::other("SID string too long"));
            }
            let units = std::slice::from_raw_parts(sid, len);
            if !units.starts_with(&[b'S' as u16, b'-' as u16])
                || !units[2..]
                    .iter()
                    .all(|u| *u == b'-' as u16 || (b'0' as u16..=b'9' as u16).contains(u))
            {
                return Err(io::Error::other("invalid SID string"));
            }
            Ok(OsString::from_wide(units))
        })();
        LocalFree(sid.cast());
        result
    }
}

/// Hold directories open without delete sharing to prevent root replacement while listing.
fn pin_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "Recycle Bin directory is not an ordinary directory",
        ));
    }
    Ok(file)
}

fn volume_id(units: &[u16]) -> io::Result<VolumeId> {
    let text = String::from_utf16(units).map_err(|_| io::Error::other("invalid volume name"))?;
    let guid = text
        .strip_prefix("\\\\?\\Volume{")
        .and_then(|s| s.strip_suffix("}\\"))
        .ok_or_else(|| io::Error::other("unexpected volume GUID path"))?;
    guid.parse()
        .map_err(|_| io::Error::other("invalid volume GUID"))
}

pub(super) fn list() -> Result<Listing, BackendError> {
    let sid = current_sid().map_err(|e| native("query process user SID", e))?;
    let mut diagnostics = Listing::default();
    let mut roots = Vec::new();
    let mut pins = Vec::new();
    let mut buffer = [0u16; 1024];
    // SAFETY: buffer is writable for the supplied length; the guard closes the search.
    let handle = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            return Ok(diagnostics);
        }
        return Err(native("enumerate local volumes", error));
    }
    let search = VolumeSearch(handle);
    loop {
        let length = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
        let path = PathBuf::from(OsString::from_wide(&buffer[..length]));
        let result = (|| -> io::Result<()> {
            let id = volume_id(&buffer[..length])?;
            // SAFETY: discovery returned a NUL-terminated volume GUID path.
            let drive_type = unsafe { GetDriveTypeW(buffer.as_ptr()) };
            if ![DRIVE_FIXED, DRIVE_REMOVABLE].contains(&drive_type) {
                diagnostics.warn(
                    "unsupported_volume",
                    &path,
                    format!("unsupported drive type {drive_type}"),
                );
                return Ok(());
            }
            let mut filesystem = [0u16; 64];
            // SAFETY: optional outputs are NULL; filesystem buffer has the supplied capacity.
            if unsafe {
                GetVolumeInformationW(
                    buffer.as_ptr(),
                    null_mut(),
                    0,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    filesystem.as_mut_ptr(),
                    filesystem.len() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let len = filesystem
                .iter()
                .position(|&u| u == 0)
                .unwrap_or(filesystem.len());
            if !String::from_utf16_lossy(&filesystem[..len]).eq_ignore_ascii_case("NTFS") {
                diagnostics.warn("unsupported_filesystem", &path, "only NTFS is supported");
                return Ok(());
            }
            let recycle = path.join("$Recycle.Bin");
            let parent_pin = match pin_directory(&recycle) {
                Ok(pin) => pin,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e),
            };
            // Never enumerate $Recycle.Bin or another SID directory.
            let directory = recycle.join(&sid);
            let user_pin = match pin_directory(&directory) {
                Ok(pin) => pin,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e),
            };
            pins.extend([parent_pin, user_pin]);
            roots.push(RecycleRoot {
                volume: id,
                directory,
            });
            Ok(())
        })();
        if let Err(error) = result {
            diagnostics.warn("volume_unreadable", &path, error);
        }
        buffer.fill(0);
        // SAFETY: the search handle remains live and the output buffer is writable.
        if unsafe { FindNextVolumeW(search.0, buffer.as_mut_ptr(), buffer.len() as u32) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                diagnostics.warn("volume_enumeration_failed", Path::new("volumes"), error);
            }
            break;
        }
    }
    let mut listing = listing::enumerate(&roots);
    listing.diagnostics.extend(diagnostics.diagnostics);
    listing.sort();
    // Explicitly keep the root handles alive until all enumeration has finished.
    drop(pins);
    Ok(listing)
}

#[path = "windows_trash.rs"]
mod trashing;
pub(super) fn trash(path: &Path, dry_run: bool) -> Result<PathBuf, BackendError> {
    trashing::trash(path, dry_run).map_err(|e| native("trash file", e))
}

pub(super) fn mutate(
    entry: &crate::listing::Entry,
    restore: Option<bool>,
    dry_run: bool,
) -> Result<(), BackendError> {
    trashing::mutate(entry, restore, dry_run).map_err(|e| native("modify Recycle Bin entry", e))
}
