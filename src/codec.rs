//! Strict, bounded in-memory codec. No filesystem access.
use crate::model::{FileTime, FormatVersion, Metadata, PathError, WindowsPath, MAX_PATH_UNITS};
use thiserror::Error;

pub const V1_RECORD_BYTES: usize = 544;
pub const MAX_RECORD_BYTES: usize = 28 + MAX_PATH_UNITS * 2;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CodecError {
    #[error("truncated metadata: need {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error("unsupported metadata version {0}")]
    UnsupportedVersion(u64),
    #[error("metadata exceeds the supported size limit")]
    TooLarge,
    #[error("invalid path length {0} (must include a path and terminating NUL)")]
    InvalidLength(u32),
    #[error("record length mismatch: expected {expected}, got {actual}")]
    InconsistentLength { expected: usize, actual: usize },
    #[error("path is missing its terminating NUL")]
    MissingTerminator,
    #[error(transparent)]
    Path(#[from] PathError),
}

fn field<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], CodecError> {
    let end = offset.checked_add(N).ok_or(CodecError::TooLarge)?;
    let bytes = data.get(offset..end).ok_or(CodecError::Truncated {
        expected: end,
        actual: data.len(),
    })?;
    let mut result = [0; N];
    result.copy_from_slice(bytes);
    Ok(result)
}

pub fn decode(data: &[u8]) -> Result<Metadata, CodecError> {
    if data.len() > MAX_RECORD_BYTES {
        return Err(CodecError::TooLarge);
    }
    let version = match u64::from_le_bytes(field(data, 0)?) {
        1 => FormatVersion::V1,
        2 => FormatVersion::V2,
        other => return Err(CodecError::UnsupportedVersion(other)),
    };
    let original_size = u64::from_le_bytes(field(data, 8)?);
    let deleted_at = FileTime(u64::from_le_bytes(field(data, 16)?));
    let (offset, expected) = match version {
        FormatVersion::V1 => (24, V1_RECORD_BYTES),
        FormatVersion::V2 => {
            let count = u32::from_le_bytes(field(data, 24)?);
            if count < 2 {
                return Err(CodecError::InvalidLength(count));
            }
            let count = usize::try_from(count).map_err(|_| CodecError::TooLarge)?;
            if count > MAX_PATH_UNITS {
                return Err(CodecError::TooLarge);
            }
            let expected = count
                .checked_mul(2)
                .and_then(|n| n.checked_add(28))
                .ok_or(CodecError::TooLarge)?;
            (28, expected)
        }
    };
    if data.len() < expected {
        return Err(CodecError::Truncated {
            expected,
            actual: data.len(),
        });
    }
    if data.len() != expected {
        return Err(CodecError::InconsistentLength {
            expected,
            actual: data.len(),
        });
    }
    let units: Vec<u16> = data[offset..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let path = match version {
        // Bytes following the first terminator in the fixed v1 buffer are unused.
        FormatVersion::V1 => {
            let end = units
                .iter()
                .position(|&c| c == 0)
                .ok_or(CodecError::MissingTerminator)?;
            &units[..end]
        }
        FormatVersion::V2 => {
            if units.last() != Some(&0) {
                return Err(CodecError::MissingTerminator);
            }
            &units[..units.len() - 1]
        }
    };
    Ok(Metadata {
        version,
        original_size,
        deleted_at,
        original_path: WindowsPath::from_utf16(path)?,
    })
}

/// Always emits v2, even when the metadata was originally decoded from v1.
pub fn encode_v2(metadata: &Metadata) -> Result<Vec<u8>, CodecError> {
    let units = metadata.original_path.as_utf16();
    let count = units.len().checked_add(1).ok_or(CodecError::TooLarge)?;
    if count > MAX_PATH_UNITS {
        return Err(CodecError::TooLarge);
    }
    let size = count
        .checked_mul(2)
        .and_then(|n| n.checked_add(28))
        .ok_or(CodecError::TooLarge)?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    bytes.extend_from_slice(&metadata.original_size.to_le_bytes());
    bytes.extend_from_slice(&metadata.deleted_at.0.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(count)
            .map_err(|_| CodecError::TooLarge)?
            .to_le_bytes(),
    );
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    Ok(bytes)
}

#[cfg(test)]
mod tests;
