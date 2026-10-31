//! Native discovery boundary. All mutation commands remain unavailable.
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("native Recycle Bin operations require Windows 10/11 on a local NTFS volume")]
    UnsupportedPlatform,
    #[error("{0} is not implemented in this read-only stage")]
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
