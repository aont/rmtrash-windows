//! Read-only enumeration over injected per-user roots, independent of the CLI.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs::{self, Metadata as FsMetadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::codec::{self, MAX_RECORD_BYTES};
use crate::model::{EntryNames, Metadata, MAX_PATH_UNITS};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VolumeId(String);

#[derive(Debug, thiserror::Error)]
#[error("invalid volume GUID or entry ID")]
pub struct InvalidId;

impl FromStr for VolumeId {
    type Err = InvalidId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 36
            || !text.bytes().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            })
        {
            return Err(InvalidId);
        }
        Ok(Self(text.to_ascii_lowercase()))
    }
}

impl fmt::Display for VolumeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An opaque key, never a filesystem path. Exact native filename units are retained.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryId {
    volume: VolumeId,
    name: Vec<u16>,
}

impl EntryId {
    pub fn new(volume: VolumeId, name: &OsStr) -> Result<Self, InvalidId> {
        let name = filename_units(name).ok_or(InvalidId)?;
        Self::from_units(volume, name)
    }

    fn from_units(volume: VolumeId, name: Vec<u16>) -> Result<Self, InvalidId> {
        if name.len() < 3
            || name.len() >= MAX_PATH_UNITS
            || name[..2] != [b'$' as u16, b'I' as u16]
            || name.iter().any(|c| [0, 47, 92, 58].contains(c))
        {
            return Err(InvalidId);
        }
        Ok(Self { volume, name })
    }

    pub fn filename_utf16(&self) -> &[u16] {
        &self.name
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wtrash1:{}:", self.volume)?;
        for unit in &self.name {
            write!(f, "{unit:04x}")?;
        }
        Ok(())
    }
}

impl FromStr for EntryId {
    type Err = InvalidId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // Bound allocations before parsing any user-provided data.
        if text.len() > 45 + MAX_PATH_UNITS * 4 {
            return Err(InvalidId);
        }
        let mut parts = text.split(':');
        if parts.next() != Some("wtrash1") {
            return Err(InvalidId);
        }
        let volume = parts.next().ok_or(InvalidId)?.parse()?;
        let hex = parts.next().ok_or(InvalidId)?;
        if parts.next().is_some() || hex.len() % 4 != 0 || !hex.is_ascii() {
            return Err(InvalidId);
        }
        let name = hex
            .as_bytes()
            .chunks_exact(4)
            .map(|chunk| {
                let chunk = std::str::from_utf8(chunk).map_err(|_| InvalidId)?;
                u16::from_str_radix(chunk, 16).map_err(|_| InvalidId)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let id = Self::from_units(volume, name)?;
        // One canonical spelling avoids ambiguous equality at CLI boundaries.
        if id.to_string() != text {
            return Err(InvalidId);
        }
        Ok(id)
    }
}

#[cfg(windows)]
fn filename_units(name: &OsStr) -> Option<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    Some(name.encode_wide().collect())
}

#[cfg(not(windows))]
fn filename_units(name: &OsStr) -> Option<Vec<u16>> {
    Some(name.to_str()?.encode_utf16().collect())
}

/// Supplied by native discovery or by tests. Never accepted from a CLI entry ID.
#[derive(Debug, Clone)]
pub struct RecycleRoot {
    pub volume: VolumeId,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug)]
pub struct Entry {
    pub id: EntryId,
    pub volume: VolumeId,
    pub metadata: Metadata,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    /// Escaped display location only; never usable as an entry ID.
    pub location: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Listing {
    pub fn warn(&mut self, code: &'static str, path: &Path, message: impl ToString) {
        self.diagnostics.push(Diagnostic {
            code,
            location: format!("{:?}", path.as_os_str()),
            message: message.to_string(),
        });
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|a, b| a.id.cmp(&b.id));
        self.diagnostics.sort();
    }

    /// Match against freshly enumerated entries; never decode a key into a path.
    /// Future mutating operations must additionally revalidate ownership and handles.
    pub fn find(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.iter().find(|entry| &entry.id == id)
    }
}

/// Reject symlinks and every Windows reparse point without following the target.
pub(crate) fn ordinary_metadata(path: &Path) -> io::Result<FsMetadata> {
    let metadata = fs::symlink_metadata(path)?;
    reject_link(&metadata)?;
    Ok(metadata)
}

fn reject_link(metadata: &FsMetadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other("reparse points are unsupported"));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other("symbolic links are unsupported"));
    }
    Ok(())
}

fn read_record(path: &Path) -> Result<Metadata, Box<dyn std::error::Error>> {
    if !ordinary_metadata(path)?.is_file() {
        return Err(io::Error::other("metadata entry is not a regular file").into());
    }
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?
    };
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    let attributes = file.metadata()?;
    reject_link(&attributes)?;
    if !attributes.is_file() {
        return Err(io::Error::other("metadata entry is not a regular file").into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(codec::decode(&bytes)?)
}

/// Missing roots are empty; failures and malformed/orphaned pairs are diagnostics.
/// No root or payload is created, traversed recursively, or modified.
pub fn enumerate(roots: &[RecycleRoot]) -> Listing {
    let mut listing = Listing::default();
    let mut seen = BTreeSet::new();
    for root in roots {
        if !seen.insert(root.volume.clone()) {
            listing.warn(
                "duplicate_volume",
                &root.directory,
                "volume supplied more than once",
            );
            continue;
        }
        if let Err(error) = enumerate_root(root, &mut listing) {
            if error.kind() != io::ErrorKind::NotFound {
                listing.warn("root_unreadable", &root.directory, error);
            }
        }
    }
    listing.sort();
    listing
}

fn enumerate_root(root: &RecycleRoot, listing: &mut Listing) -> io::Result<()> {
    if !ordinary_metadata(&root.directory)?.is_dir() {
        return Err(io::Error::other("Recycle Bin root is not a directory"));
    }
    let mut names = BTreeMap::<OsString, PathBuf>::new();
    for item in fs::read_dir(&root.directory)? {
        match item {
            Ok(item) => {
                names.insert(item.file_name(), item.path());
            }
            Err(error) => listing.warn("entry_unreadable", &root.directory, error),
        }
    }
    for (name, path) in &names {
        let Ok(pair) = EntryNames::from_name(name) else {
            continue;
        };
        if name == &pair.payload {
            if !names.contains_key(&pair.metadata) {
                listing.warn("orphan_payload", path, "no matching $I metadata");
            }
            continue;
        }
        let Some(payload) = names.get(&pair.payload) else {
            listing.warn("orphan_metadata", path, "no matching $R payload");
            continue;
        };
        let result = (|| -> Result<Entry, Box<dyn std::error::Error>> {
            let metadata = read_record(path)?;
            let attributes = ordinary_metadata(payload)?;
            let kind = if attributes.is_dir() {
                EntryKind::Directory
            } else if attributes.is_file() {
                EntryKind::File
            } else {
                return Err(io::Error::other("unsupported payload type").into());
            };
            Ok(Entry {
                id: EntryId::new(root.volume.clone(), name)?,
                volume: root.volume.clone(),
                metadata,
                kind,
            })
        })();
        match result {
            Ok(entry) => listing.entries.push(entry),
            Err(error) => listing.warn("entry_skipped", path, error),
        }
    }
    Ok(())
}

/// Escapes controls so one path cannot add rows or terminal escape sequences.
pub(crate) fn display_path(units: &[u16]) -> String {
    let mut output = String::new();
    for c in char::decode_utf16(units.iter().copied()) {
        match c {
            Ok(c) if !c.is_control() => output.push(c),
            Ok(c) => {
                let _ = write!(output, "\\u{{{:04x}}}", u32::from(c));
            }
            Err(c) => {
                let _ = write!(output, "\\u{{{:04x}}}", c.unpaired_surrogate());
            }
        }
    }
    output
}

#[cfg(test)]
mod tests;
