use super::*;
use crate::model::{FileTime, FormatVersion, WindowsPath};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const VOLUME_A: &str = "12345678-1234-1234-1234-123456789abc";
const VOLUME_B: &str = "87654321-1234-1234-1234-123456789abc";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wtrash-list-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // create_dir refuses to reuse an existing tree.
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self, volume: &str) -> RecycleRoot {
        let directory = self.0.join(volume);
        fs::create_dir(&directory).unwrap();
        RecycleRoot {
            volume: volume.parse().unwrap(),
            directory,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pair(root: &RecycleRoot, name: &OsStr, path: &str, directory: bool) {
    let names = EntryNames::from_name(name).unwrap();
    let metadata = Metadata {
        version: FormatVersion::V2,
        original_size: u64::MAX,
        deleted_at: FileTime(FileTime::UNIX_EPOCH_TICKS + 1),
        original_path: WindowsPath::from_utf16(&path.encode_utf16().collect::<Vec<_>>()).unwrap(),
    };
    fs::write(
        root.directory.join(names.metadata),
        codec::encode_v2(&metadata).unwrap(),
    )
    .unwrap();
    let payload = root.directory.join(names.payload);
    if directory {
        fs::create_dir(payload).unwrap();
    } else {
        fs::write(payload, b"payload").unwrap();
    }
}

#[test]
fn multiple_volumes_duplicate_paths_and_deterministic_output() {
    let fixture = Fixture::new();
    let a = fixture.root(VOLUME_A);
    let b = fixture.root(VOLUME_B);
    pair(&a, OsStr::new("$I同じ.txt"), "C:\\日本語\\same.txt", false);
    pair(&b, OsStr::new("$I同じ.txt"), "C:\\日本語\\same.txt", true);
    pair(&a, OsStr::new("$Isecond"), "C:\\日本語\\same.txt", false);
    let forward = enumerate(&[a.clone(), b.clone()]);
    let reverse = enumerate(&[b, a]);
    assert_eq!(forward.entries.len(), 3);
    assert!(forward.diagnostics.is_empty());
    let ids: Vec<_> = forward.entries.iter().map(|e| e.id.to_string()).collect();
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(
        ids,
        reverse
            .entries
            .iter()
            .map(|e| e.id.to_string())
            .collect::<Vec<_>>()
    );
    assert!(forward
        .entries
        .iter()
        .any(|e| e.kind == EntryKind::Directory));
    for entry in &forward.entries {
        let parsed: EntryId = entry.id.to_string().parse().unwrap();
        assert_eq!(forward.find(&parsed).unwrap().id, entry.id);
    }
    let mut json = Vec::new();
    crate::presentation::write_json(&forward, &mut json).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["entries"][0]["original_size"], u64::MAX.to_string());
    assert_eq!(
        doc["entries"][0]["deleted_at_utc"],
        "1970-01-01T00:00:00.000000100Z"
    );
    let mut second = Vec::new();
    crate::presentation::write_json(&reverse, &mut second).unwrap();
    assert_eq!(json, second);
}

#[test]
fn orphans_corrupt_and_oversized_records_do_not_hide_valid_entries() {
    let fixture = Fixture::new();
    let root = fixture.root(VOLUME_A);
    pair(&root, OsStr::new("$Ivalid"), "C:\\valid", false);
    pair(&root, OsStr::new("$Ibad"), "C:\\bad", false);
    fs::write(root.directory.join("$Ibad"), b"corrupt").unwrap();
    pair(&root, OsStr::new("$Ihuge"), "C:\\huge", false);
    fs::write(root.directory.join("$Ihuge"), vec![0; MAX_RECORD_BYTES + 1]).unwrap();
    fs::write(root.directory.join("$Ialone"), b"orphan").unwrap();
    fs::write(root.directory.join("$Ralone-payload"), b"orphan").unwrap();
    fs::write(root.directory.join("desktop.ini"), b"ignored").unwrap();
    let listing = enumerate(&[root]);
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.diagnostics.len(), 4);
    assert_eq!(
        listing
            .diagnostics
            .iter()
            .filter(|d| d.code == "entry_skipped")
            .count(),
        2
    );
    assert!(listing
        .diagnostics
        .iter()
        .any(|d| d.code == "orphan_metadata"));
    assert!(listing
        .diagnostics
        .iter()
        .any(|d| d.code == "orphan_payload"));
}

#[test]
fn empty_missing_and_unreadable_roots() {
    let fixture = Fixture::new();
    let empty = fixture.root(VOLUME_A);
    let listing = enumerate(&[empty]);
    assert!(listing.entries.is_empty());
    assert!(listing.diagnostics.is_empty());
    let root = RecycleRoot {
        volume: VOLUME_B.parse().unwrap(),
        directory: fixture.0.join("missing"),
    };
    assert!(enumerate(&[root.clone()]).diagnostics.is_empty());
    // A regular file makes directory enumeration fail even under elevated test users.
    fs::write(&root.directory, b"not a directory").unwrap();
    let listing = enumerate(&[root]);
    assert!(listing.entries.is_empty());
    assert_eq!(listing.diagnostics[0].code, "root_unreadable");
}

#[cfg(unix)]
#[test]
fn permission_denied_root_is_a_diagnostic_when_permissions_are_enforced() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let root = fixture.root(VOLUME_A);
    fs::set_permissions(&root.directory, fs::Permissions::from_mode(0o0)).unwrap();
    let denied = fs::read_dir(&root.directory).is_err();
    let listing = enumerate(&[root.clone()]);
    fs::set_permissions(&root.directory, fs::Permissions::from_mode(0o700)).unwrap();
    // Elevated users may bypass mode bits; the unreadable-root test is unconditional.
    if denied {
        assert_eq!(listing.diagnostics[0].code, "root_unreadable");
    }
}

#[test]
fn ids_reject_paths_and_noncanonical_spellings() {
    let id = EntryId::new(VOLUME_A.parse().unwrap(), OsStr::new("$Iabcdef.txt")).unwrap();
    assert_eq!(id.to_string().parse::<EntryId>().unwrap(), id);
    for value in [
        "",
        "0",
        "C:\\$Recycle.Bin\\$Iabcdef",
        "../$Iabcdef",
        "wtrash1:bad:00240049",
        "wtrash1:12345678-1234-1234-1234-123456789abc:00240049002f0061",
    ] {
        assert!(value.parse::<EntryId>().is_err(), "{value}");
    }
    assert!(id.to_string().to_uppercase().parse::<EntryId>().is_err());
    for name in [
        "$I../escape",
        "$I..\\escape",
        "$Ifile:stream",
        "$I",
        "$Rfile",
    ] {
        assert!(EntryId::new(VOLUME_A.parse().unwrap(), OsStr::new(name)).is_err());
    }
}

#[test]
fn human_paths_escape_terminal_controls() {
    let fixture = Fixture::new();
    let root = fixture.root(VOLUME_A);
    pair(
        &root,
        OsStr::new("$Icontrol"),
        "C:\\line\n\t\u{1b}path",
        false,
    );
    let mut output = Vec::new();
    crate::presentation::write_human(&enumerate(&[root]), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("\\u{000a}"));
}

#[cfg(unix)]
#[test]
fn symlink_roots_and_payloads_are_not_followed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let root = fixture.root(VOLUME_A);
    pair(&root, OsStr::new("$Ilink"), "C:\\link", false);
    let payload = root.directory.join("$Rlink");
    fs::remove_file(&payload).unwrap();
    symlink(&fixture.0, &payload).unwrap();
    let listing = enumerate(&[root.clone()]);
    assert!(listing.entries.is_empty());
    assert_eq!(listing.diagnostics[0].code, "entry_skipped");
    let alias = fixture.0.join("alias");
    symlink(&root.directory, &alias).unwrap();
    let listing = enumerate(&[RecycleRoot {
        directory: alias,
        ..root
    }]);
    assert_eq!(listing.diagnostics[0].code, "root_unreadable");
}

#[cfg(windows)]
#[test]
fn native_non_unicode_entry_names_retain_exact_identity() {
    use std::os::windows::ffi::OsStringExt;
    let fixture = Fixture::new();
    let root = fixture.root(VOLUME_A);
    let name = OsString::from_wide(&[0x24, 0x49, 0xd800, 0x61]);
    pair(&root, &name, "C:\\valid.txt", false);
    let listing = enumerate(&[root]);
    assert_eq!(listing.entries.len(), 1);
    let id = &listing.entries[0].id;
    assert_eq!(id.filename_utf16(), &[0x24, 0x49, 0xd800, 0x61]);
    assert_eq!(id.to_string().parse::<EntryId>().unwrap(), *id);
    let mut output = Vec::new();
    crate::presentation::write_json(&listing, &mut output).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["entries"][0]["entry_name_utf16"][2], 0xd800);
}
