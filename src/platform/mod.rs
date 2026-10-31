//! Native discovery boundary. File trashing uses a separate staged transaction.
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("native Recycle Bin operations require Windows 10/11 on a local NTFS volume")]
    UnsupportedPlatform,
    #[error("{0} is not implemented in this stage")]
    NotImplemented(&'static str),
    #[error("{operation}: {source}")]
    Native {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

pub fn list() -> Result<crate::listing::Listing, BackendError> {
    #[cfg(windows)]
    {
        windows::list()
    }
    #[cfg(not(windows))]
    {
        Err(BackendError::UnsupportedPlatform)
    }
}

pub fn unavailable(operation: &'static str) -> BackendError {
    #[cfg(windows)]
    {
        BackendError::NotImplemented(operation)
    }
    #[cfg(not(windows))]
    {
        let _ = operation;
        BackendError::UnsupportedPlatform
    }
}

pub fn trash(path: &std::path::Path, dry_run: bool) -> Result<std::path::PathBuf, BackendError> {
    #[cfg(windows)]
    {
        windows::trash(path, dry_run)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, dry_run);
        Err(BackendError::UnsupportedPlatform)
    }
}
