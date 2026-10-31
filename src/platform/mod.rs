//! Platform boundary. No discovery, reads, writes, or native API calls in stage 1.
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("native Recycle Bin operations require Windows 10/11 on a local NTFS volume")]
    UnsupportedPlatform,
    #[error(
        "{0} is not implemented in this foundation stage; no Recycle Bin access was performed"
    )]
    NotImplemented(&'static str),
}

/// Gate all CLI operations before any filesystem access.
pub fn unavailable(operation: &'static str) -> BackendError {
    #[cfg(windows)]
    {
        windows::unavailable(operation)
    }
    #[cfg(not(windows))]
    {
        let _ = operation;
        BackendError::UnsupportedPlatform
    }
}
