//! Two-file commit protocol, independent of metadata encoding and native APIs.
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct Names {
    pub metadata: PathBuf,
    pub payload: PathBuf,
    pub stage: PathBuf,
}

/// Operations must act on pinned objects and refuse replacement atomically.
/// A failed operation must leave the named object at its previous location.
pub(crate) trait Operations {
    type Stage;
    fn names(&mut self) -> io::Result<Names>;
    fn create(&mut self, names: &Names) -> io::Result<Self::Stage>;
    fn write(&mut self, stage: &mut Self::Stage, bytes: &[u8]) -> io::Result<()>;
    fn move_payload(&mut self, names: &Names) -> io::Result<()>;
    fn publish(&mut self, stage: &Self::Stage, names: &Names) -> io::Result<()>;
    fn rollback(&mut self) -> io::Result<()>;
    fn cleanup(&mut self, stage: &Self::Stage) -> io::Result<()>;
}

fn cleanup<O: Operations>(ops: &mut O, stage: &O::Stage, names: &Names) -> io::Result<()> {
    ops.cleanup(stage).map_err(|e| {
        io::Error::other(format!(
            "staged metadata cleanup failed: {e}; inspect/remove {:?}; payload is at its original path",
            names.stage
        ))
    })
}

pub(crate) fn commit<O: Operations>(ops: &mut O, bytes: &[u8]) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let names = ops.names()?;
        let mut stage = match ops.create(&names) {
            Ok(stage) => stage,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if let Err(e) = ops.write(&mut stage, bytes) {
            cleanup(ops, &stage, &names)?;
            return Err(io::Error::other(format!("stage metadata: {e}")));
        }
        if let Err(e) = ops.move_payload(&names) {
            cleanup(ops, &stage, &names)?;
            if e.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(io::Error::other(format!("move payload: {e}")));
        }
        if let Err(e) = ops.publish(&stage, &names) {
            if let Err(rollback) = ops.rollback() {
                // Retain BOTH payload and complete staged metadata for recovery.
                return Err(io::Error::other(format!(
                    "publish metadata: {e}; rollback failed: {rollback}; payload preserved at {:?}; metadata preserved at {:?}. Close programs using the file, then move the payload to the original path if vacant, or to a new safe path. Do not delete the payload",
                    names.payload, names.stage
                )));
            }
            cleanup(ops, &stage, &names)?;
            if e.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(io::Error::other(format!(
                "publish metadata: {e}; source restored"
            )));
        }
        return Ok(names.metadata);
    }
    Err(io::Error::other(
        "128 name collisions; source remains at its original path",
    ))
}

#[cfg(test)]
mod tests;
