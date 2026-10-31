//! Native discovery boundary. File trashing uses a separate staged transaction.
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("native Recycle Bin operations require Windows 10/11 on a local NTFS volume")]
    UnsupportedPlatform,
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

/// Restore (Some(create_parents)) or permanently remove (None) a fresh selection.
pub fn mutate(
    entry: &crate::listing::Entry,
    restore: Option<bool>,
    dry_run: bool,
) -> Result<(), BackendError> {
    #[cfg(windows)]
    {
        windows::mutate(entry, restore, dry_run)
    }
    #[cfg(not(windows))]
    {
        let _ = (entry, restore, dry_run);
        Err(BackendError::UnsupportedPlatform)
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
