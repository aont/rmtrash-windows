use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// Includes the on-disk terminating NUL. Bounds all codec path allocations.
pub const MAX_PATH_UNITS: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path exceeds the supported UTF-16 code-unit limit")]
    TooLong,
    #[error("path contains an embedded NUL")]
    EmbeddedNul,
    #[error("path contains malformed UTF-16")]
    InvalidUtf16,
    #[error("host path cannot be represented as Unicode")]
    InvalidHostPath,
    #[error("expected a single $I or $R filename with a nonempty suffix")]
    InvalidPairName,
}

/// Validated UTF-16, preserved without UTF-8 normalization or replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsPath(Vec<u16>);

impl WindowsPath {
    pub fn from_utf16(units: &[u16]) -> Result<Self, PathError> {
        if units.is_empty() {
            return Err(PathError::Empty);
        }
        if units.len() >= MAX_PATH_UNITS {
            return Err(PathError::TooLong);
        }
        if units.contains(&0) {
            return Err(PathError::EmbeddedNul);
        }
        if char::decode_utf16(units.iter().copied()).any(|c| c.is_err()) {
            return Err(PathError::InvalidUtf16);
        }
        Ok(Self(units.to_vec()))
    }

    pub fn from_path(path: &Path) -> Result<Self, PathError> {
        Self::from_utf16(&os_units(path.as_os_str())?)
    }

    pub fn as_utf16(&self) -> &[u16] {
        &self.0
    }

    pub fn to_path_buf(&self) -> Result<PathBuf, PathError> {
        Ok(PathBuf::from(units_os(&self.0)?))
    }
}

#[cfg(windows)]
fn os_units(value: &OsStr) -> Result<Vec<u16>, PathError> {
    use std::os::windows::ffi::OsStrExt;
    // Stop before allocating an arbitrarily large native path.
    let units: Vec<_> = value.encode_wide().take(MAX_PATH_UNITS).collect();
    if units.len() == MAX_PATH_UNITS {
        return Err(PathError::TooLong);
    }
    Ok(units)
}

#[cfg(not(windows))]
fn os_units(value: &OsStr) -> Result<Vec<u16>, PathError> {
    let text = value.to_str().ok_or(PathError::InvalidHostPath)?;
    let units: Vec<_> = text.encode_utf16().take(MAX_PATH_UNITS).collect();
    if units.len() == MAX_PATH_UNITS {
        return Err(PathError::TooLong);
    }
    Ok(units)
}

#[cfg(windows)]
fn units_os(units: &[u16]) -> Result<OsString, PathError> {
    use std::os::windows::ffi::OsStringExt;
    Ok(OsString::from_wide(units))
}

#[cfg(not(windows))]
fn units_os(units: &[u16]) -> Result<OsString, PathError> {
    String::from_utf16(units)
        .map(OsString::from)
        .map_err(|_| PathError::InvalidUtf16)
}

/// Unsigned 100 ns ticks since 1601-01-01 UTC. Raw values are never rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTime(pub u64);

impl FileTime {
    pub const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

    pub fn to_system_time(self) -> Option<SystemTime> {
        let ticks = self.0.abs_diff(Self::UNIX_EPOCH_TICKS);
        let duration = Duration::new(ticks / 10_000_000, ((ticks % 10_000_000) * 100) as u32);
        if self.0 >= Self::UNIX_EPOCH_TICKS {
            UNIX_EPOCH.checked_add(duration)
        } else {
            UNIX_EPOCH.checked_sub(duration)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub version: FormatVersion,
    pub original_size: u64,
    pub deleted_at: FileTime,
    pub original_path: WindowsPath,
}

/// Only a filename pair, never a path authorized for filesystem access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryNames {
    pub metadata: OsString,
    pub payload: OsString,
}

impl EntryNames {
    pub fn from_name(name: &OsStr) -> Result<Self, PathError> {
        let mut units = os_units(name)?;
        if units.len() < 3
            || units[0] != u16::from(b'$')
            || ![u16::from(b'I'), u16::from(b'R')].contains(&units[1])
            || units.iter().any(|c| [0, 47, 92, 58].contains(c))
        {
            return Err(PathError::InvalidPairName);
        }
        units[1] = u16::from(b'I');
        let metadata = units_os(&units)?;
        units[1] = u16::from(b'R');
        Ok(Self {
            metadata,
            payload: units_os(&units)?,
        })
    }
}
