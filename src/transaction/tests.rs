//! Synthetic directories only: never calls production Windows discovery.
use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    directory: PathBuf,
    attempt: usize,
    fail: &'static str,
    events: Vec<&'static str>,
}
impl Fixture {
    fn new(fail: &'static str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "wtrash-transaction-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("source"), b"only payload").unwrap();
        Self {
            directory,
            attempt: 0,
            fail,
            events: vec![],
        }
    }
    fn original(&self) -> PathBuf {
        self.directory.join("source")
    }
    fn inject(&mut self, event: &'static str) -> io::Result<()> {
        self.events.push(event);
        if self.fail == event
            || (self.fail == "rollback" && event == "publish")
            || (self.fail == "cleanup" && event == "write")
        {
            Err(io::Error::other(format!("injected {event}")))
        } else {
            Ok(())
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}
// Fixture-only no-clobber movement: the production backend uses handle renames.
fn move_fixture(source: &std::path::Path, destination: &std::path::Path) -> io::Result<()> {
    fs::hard_link(source, destination)?;
    fs::remove_file(source)
}
impl Operations for Fixture {
    type Stage = PathBuf;
    fn names(&mut self) -> io::Result<Names> {
        self.attempt += 1;
        let names = Names {
            metadata: self.directory.join(format!("$I{:06}", self.attempt)),
            payload: self.directory.join(format!("$R{:06}", self.attempt)),
            stage: self.directory.join(format!("stage{}", self.attempt)),
        };
        if self.attempt == 1 {
            let collision = match self.fail {
                "stage_collision" => Some(&names.stage),
                "payload_collision" => Some(&names.payload),
                _ => None,
            };
            if let Some(path) = collision {
                fs::write(path, b"other operation").unwrap();
            }
        }
        if self.fail == "exhaustion" {
            fs::write(&names.stage, b"other operation").unwrap();
        }
        Ok(names)
    }
    fn create(&mut self, names: &Names) -> io::Result<PathBuf> {
        self.inject("create")?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&names.stage)?;
        Ok(names.stage.clone())
    }
    fn write(&mut self, stage: &mut PathBuf, bytes: &[u8]) -> io::Result<()> {
        fs::write(&*stage, &bytes[..1])?; // Simulate a partially written record on failure.
        self.inject("write")?;
        fs::write(stage, bytes)
    }
    fn move_payload(&mut self, names: &Names) -> io::Result<()> {
        self.inject("move")?;
        move_fixture(&self.original(), &names.payload)
    }
    fn publish(&mut self, stage: &PathBuf, names: &Names) -> io::Result<()> {
        self.inject("publish")?;
        assert_eq!(fs::read(stage).unwrap(), b"complete metadata");
        assert!(!self.original().exists());
        // Inject a concurrent publication after payload movement, rather than an
        // existence precheck. Publication must refuse to replace its bytes.
        if self.fail == "publication_collision" && self.attempt == 1 {
            fs::write(&names.metadata, b"other operation")?;
        }
        if self.fail == "rollback" {
            unreachable!();
        }
        move_fixture(stage, &names.metadata)
    }
    fn rollback(&mut self) -> io::Result<()> {
        if self.fail == "rollback" {
            fs::write(self.original(), b"new source from another process")?;
        }
        self.inject("rollback")?;
        move_fixture(
            &self.directory.join(format!("$R{:06}", self.attempt)),
            &self.original(),
        )
    }
    fn cleanup(&mut self, stage: &PathBuf) -> io::Result<()> {
        self.inject("cleanup")?;
        fs::remove_file(stage)
    }
}

#[test]
fn success_publishes_complete_metadata_after_payload() {
    let mut f = Fixture::new("");
    let path = commit(&mut f, b"complete metadata").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"complete metadata");
    assert_eq!(
        fs::read(f.directory.join("$R000001")).unwrap(),
        b"only payload"
    );
    assert!(!f.original().exists());
    assert_eq!(f.events, ["create", "write", "move", "publish"]);
}

#[test]
fn injected_failures_restore_source_and_leave_no_valid_entry() {
    for event in ["create", "write", "move", "publish"] {
        let mut f = Fixture::new(event);
        assert!(commit(&mut f, b"complete metadata").is_err());
        assert_eq!(fs::read(f.original()).unwrap(), b"only payload");
        assert_eq!(fs::read_dir(&f.directory).unwrap().count(), 1, "{event}");
    }
}

#[test]
fn concurrent_collisions_retry_without_replacing_other_entries() {
    for (event, survivor) in [
        ("stage_collision", "stage1"),
        ("payload_collision", "$R000001"),
        ("publication_collision", "$I000001"),
    ] {
        let mut f = Fixture::new(event);
        let path = commit(&mut f, b"complete metadata").unwrap();
        assert!(path.ends_with("$I000002"));
        assert_eq!(
            fs::read(f.directory.join(survivor)).unwrap(),
            b"other operation"
        );
        assert_eq!(
            fs::read(f.directory.join("$R000002")).unwrap(),
            b"only payload"
        );
        assert!(!f.directory.join("stage2").exists());
    }
}

#[test]
fn failed_rollback_preserves_payload_metadata_and_new_source() {
    let mut f = Fixture::new("rollback");
    let error = commit(&mut f, b"complete metadata")
        .unwrap_err()
        .to_string();
    assert!(error.contains("payload preserved at"));
    assert!(error.contains("metadata preserved at"));
    assert_eq!(
        fs::read(f.directory.join("$R000001")).unwrap(),
        b"only payload"
    );
    assert_eq!(
        fs::read(f.directory.join("stage1")).unwrap(),
        b"complete metadata"
    );
    assert_eq!(
        fs::read(f.original()).unwrap(),
        b"new source from another process"
    );
    assert!(!f.directory.join("$I000001").exists());
}

#[test]
fn cleanup_failure_reports_stage_and_keeps_source() {
    let mut f = Fixture::new("cleanup");
    let error = commit(&mut f, b"complete metadata")
        .unwrap_err()
        .to_string();
    assert!(error.contains("stage1"));
    assert_eq!(fs::read(f.original()).unwrap(), b"only payload");
}

#[test]
fn bounded_collisions_never_move_source() {
    let mut f = Fixture::new("exhaustion");
    assert!(commit(&mut f, b"complete metadata")
        .unwrap_err()
        .to_string()
        .contains("128"));
    assert_eq!(fs::read(f.original()).unwrap(), b"only payload");
    assert!(!f.events.contains(&"move"));
}
