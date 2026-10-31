use super::BackendError;

// Future ordinary Win32 filesystem/volume/security implementation belongs here.
// No Shell/COM interfaces or external utility fallbacks are permitted.
pub(super) fn unavailable(operation: &'static str) -> BackendError {
    BackendError::NotImplemented(operation)
}
