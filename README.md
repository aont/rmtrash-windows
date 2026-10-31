# wtrash

A Windows 10/11 Recycle Bin CLI with file trashing, listing, restoration, and
confirmed permanent deletion through direct `$I` metadata and `$R` payload access.
No Shell Recycle Bin APIs, COM, PowerShell, Explorer automation, or external trash utilities are used.

The on-disk layout is an undocumented implementation detail. Windows compilation
has been checked, but live Windows behavior and Explorer compatibility have not
been verified. See the manual verification procedure below.

## CLI

```text
wtrash trash [--dry-run] <PATH>...
wtrash list [--json]
wtrash restore [--create-parents] [--dry-run] <ENTRY_ID>...
wtrash remove [--yes] [--dry-run] <ENTRY_ID>...
wtrash empty [--yes] [--dry-run]
wtrash <PATH>...                 # shorthand for trash
```

Help and version output are available through `--help` and `--version`.
`list` prints tab-separated columns: entry ID, original path, deletion time in UTC,
original size in bytes, file/directory type, and source volume GUID. Control
characters in human-readable paths are escaped. An empty listing prints only the
header. `list --json` emits a JSON document even when no entries are found.

A successful discovery returns status 0, including partial results with warnings.
Inspect diagnostics when completeness matters. Human output puts warnings on
stderr; JSON embeds them in `diagnostics` without contaminating stdout. Failure
to query the process SID or start volume discovery returns status 1. Inaccessible
individual volumes, malformed entries, and later discovery failures produce
warnings and preserve other results. Missing `$Recycle.Bin` or SID directories
are empty and never created. Invalid arguments return status 2. On non-Windows
hosts, all operations fail with an explicit Windows-required error.
Trashing and mutations process every selected input,
report each result, and return status 1 if any input fails. On non-Windows
hosts it fails before accessing files.

Arguments use `PathBuf` or `OsString`. A path matching a command name must be
qualified (for example, `.\list`); prefix option-like paths with `--`.

## JSON contract (schema version 1)

```json
{
  "schema_version": 1,
  "entries": [],
  "diagnostics": []
}
```

Every entry has these fields:

| Field | Type and meaning |
| --- | --- |
| `id` | Opaque canonical entry identifier described below |
| `volume` | Lowercase volume GUID without braces |
| `entry_name_utf16` | Array of exact native `$I` filename code units, including unpaired surrogates; no NUL |
| `original_path` | Original path as a Unicode string, without normalization |
| `original_path_utf16` | Exact decoded original path UTF-16 code units; no NUL |
| `original_size` | Decimal string of original size in bytes |
| `deleted_at_filetime` | Decimal string of raw unsigned FILETIME ticks |
| `deleted_at_utc` | UTC RFC 3339 string with nine fractional digits, or null if outside the RFC 3339 year range |
| `kind` | `"file"` or `"directory"`, determined from the `$R` payload |

Unsigned 64-bit values are decimal **strings** to avoid precision loss in script
runtimes. Paths containing malformed UTF-16 metadata are rejected by the existing
codec and reported; no replacement Unicode changes their identity. Native entry
filenames may contain unpaired surrogates: IDs and `entry_name_utf16` preserve
those units without conversion to Unicode strings.

Each diagnostic has string fields `code`, `location`, and `message`. `location`
is an escaped display path, never an operational identifier. Codes include
`unsupported_volume`, `unsupported_filesystem`, `volume_unreadable`,
`volume_enumeration_failed`, `duplicate_volume`, `root_unreadable`,
`entry_unreadable`, `orphan_metadata`, `orphan_payload`, and `entry_skipped`.
Messages may contain OS-localized text. Entries sort by volume GUID then exact
UTF-16 filename; diagnostics sort by code, location, then message. Ordering is
deterministic for a fixed filesystem state.

## Entry identifiers and discovery limits

An ID is `wtrash1:<volume-guid>:<filename-utf16-hex>`. Each UTF-16 unit of the
actual `$I` filename is encoded as exactly four lowercase hex digits, including
`$I` itself. For example, `$Ia` ends in `:002400490061`. This distinguishes
identically named entries across volumes and multiple deleted versions of an
original path. IDs do not depend on drive letters or list positions.

Parsing accepts only canonical IDs with a valid GUID and a single `$I` filename:
NUL, slash, backslash, colon, empty suffixes, and overlong inputs are rejected.
`Listing::find` compares a parsed ID against freshly enumerated entries. The CLI
resolves IDs through discovery before joining exact validated entry names to the
current SID root. Mutations revalidate handles and metadata before acting. IDs are
locators, not capabilities or immutable object identities: a removed name may
be reused, cloned volumes may share identities, and volume GUIDs are specific
to the Windows installation and may change after reconfiguration.

Discovery uses `OpenProcessToken` / `GetTokenInformation(TokenUser)` and
`ConvertSidToStringSidW`, never an environment-supplied username or SID. It uses
`FindFirstVolumeW` / `FindNextVolumeW` volume GUID paths, `GetDriveTypeW`, and
`GetVolumeInformationW`. Drive letters are unnecessary, so NTFS volumes mounted
only in directories are included when Windows exposes their GUID paths. Network
shares/mapped network drives, non-NTFS filesystems, unknown drive types, optical
media, RAM disks, and legacy INFO/INFO2 layouts are excluded. Offline, locked,
or inaccessible volumes may yield warnings. Removable NTFS volumes are eligible,
although Windows often has no Recycle Bin on removable media.

Only `<volume>\$Recycle.Bin\<process-SID>` is enumerated; the parent and other
users' directories are never enumerated. Reparse points in the Recycle Bin
parent, SID root, metadata, or payload are rejected. Native directory handles
remain open without delete sharing during enumeration to prevent replacement of
the parent or SID root; metadata opens also reject final-component reparse points.
Payload directories are identified without reading their contents. Orphaned `$I`
and `$R` entries are diagnosed and excluded; corrupted or unreadable pairs do not
hide valid entries. This is a live, non-atomic view: concurrent recycling or
cleanup can change entries and produce skipped-pair warnings.

## Modules

- `src/main.rs`: argument parsing and output/error handling.
- `src/model.rs`: metadata, raw FILETIME, validated Windows paths, filename pairs.
- `src/codec.rs`: bounded metadata codec without I/O.
- `src/listing.rs`: IDs, injectable per-user roots, read-only enumeration/diagnostics.
- `src/platform/windows.rs`: process SID and volume discovery, pinned native roots.
- `src/platform/windows_trash.rs`: validation, native moves, restore and deletion.
- `src/transaction.rs`: injectable two-file commit and recovery protocol.
- `src/restore.rs`: conservative DOS destination validation.
- `src/presentation.rs`: human and versioned JSON serialization.
- `src/codec/tests.rs`, `src/listing/tests.rs`: codec and synthetic filesystem fixtures.

No Shell/COM Recycle Bin interface, shell-backed recycling crate, PowerShell,
Explorer, or external trash utility is used. Tests can supply `RecycleRoot`
values directly to `listing::enumerate`; there is deliberately no CLI switch or
environment variable redirecting production discovery or trashing to an arbitrary
user root.

## Metadata format

All integer fields are little-endian. Each `$I<suffix>` metadata file corresponds
to `$R<suffix>` payload data in the same per-volume, per-user directory,
commonly `<volume>\$Recycle.Bin\<SID>`. Pairing preserves the complete suffix,
including any extension; it never assumes a six-character identifier. Pairing
accepts a single filename only and does not authorize filesystem access or
validate a future user-facing entry ID.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | Version: 1 or 2 |
| 8 | 8 | Original size in bytes, retained as `u64` |
| 16 | 8 | Deletion FILETIME: 100 ns ticks since 1601-01-01 UTC |
| 24 (v1) | 520 | Fixed UTF-16LE path buffer, including NUL |
| 24 (v2) | 4 | UTF-16 code-unit count, **including the final NUL** |
| 28 (v2) | count × 2 | UTF-16LE path and final NUL |

Version 1 records must be exactly 544 bytes. The first NUL ends the path; unused
fixed-buffer bytes after it are ignored. Version 2 records must exactly match
the declared length, with exactly one final terminator and no embedded NUL.
A surrogate pair occupies two code units. Empty paths, malformed UTF-16,
unsupported versions, missing terminators, truncated headers or paths, and
inconsistent lengths produce explicit typed errors. `encode_v2` always emits
version 2, including when converting metadata originally read as version 1.

The codec allows at most 32,768 path code units including NUL (65,564 bytes per
v2 record). It checks lengths and arithmetic before allocating, with no allocation
based on an unchecked on-disk count. This is a parser resource limit, not a claim
that every path of that length is supported by Windows filesystem operations.
The codec does not normalize paths or validate absolute paths, volume membership,
reparse points, or restore destinations.

`WindowsPath` stores validated UTF-16 code units. On Windows it converts directly
through `OsStrExt::encode_wide` and `OsStringExt::from_wide` to/from native paths.
On other hosts it uses strict Unicode conversions for portable codec work,
without interpreting Windows separators as native filesystem syntax. Unpaired
surrogates in metadata paths are explicitly rejected, never replaced; such NT
filenames are currently unsupported. Native filename pairing on Windows preserves
UTF-16 even for non-Unicode suffixes. FILETIME values remain raw and lossless;
`to_system_time()` uses checked arithmetic and returns `None` if the host cannot
represent a timestamp, including dates before the Unix epoch.

## Trashing safety and transaction limits

`wtrash trash --dry-run <PATH>...` resolves paths, opens/pins the source and
ancestors, validates volume/SID availability, and serializes metadata in memory.
It prints the target SID directory and creates or moves nothing. It does not
reserve names or guarantee that a future write will succeed. Shorthand accepts
multiple paths too. Use `wtrash trash -- --dry-run` to trash a file literally
named `--dry-run`, and `wtrash -- list` for a file literally named `list`.

Supported inputs are ordinary, singly linked files on local fixed/removable
NTFS volumes with an existing Windows-initialized `$Recycle.Bin` and current
process SID directory. Relative and drive-qualified absolute DOS paths, spaces,
Unicode, extensionless files, and long paths within the codec limit are accepted
when native filesystem calls support them. Native UTF-16 is preserved without
lossy conversion; malformed UTF-16 is explicitly unsupported by the codec.
Extensions are retained in `$I`/`$R` names and must fit a 255-unit component.
Directories, volume roots, UNC/mapped network paths, device/extended namespace
inputs, alternate data streams, trailing-dot/space components, multiple hard
links, and all source/ancestor/root reparse points are rejected. This includes
symlinks, junctions, volume mount-point ancestors, and cloud placeholders. Short
aliases are resolved before rejecting paths inside `$Recycle.Bin`. A resolved
source must have a DOS drive path. Files open with conflicting sharing modes may
fail safely. No directories or ACLs are initialized or modified; no quotas or
eviction are implemented.

Source/ancestor and Recycle Bin directory handles stay open without delete
sharing to prevent path replacement. The source also denies write sharing while
its size is collected and the operation runs. Each attempt uses the system RNG
to generate a six-character uppercase alphanumeric suffix, retaining the source
extension. A `.wtrash-<suffix>.tmp` file is created exclusively in the SID root;
its complete metadata is written and flushed before payload movement. Both
payload movement and final metadata publication use `SetFileInformationByHandle`
with replacement disabled. Existing names are never overwritten, and collision
handling retries up to 128 times. There is no copy/delete fallback, including on
cross-volume errors. Staging also excludes competing wtrash attempts choosing
the same suffix. A precheck avoids existing metadata; concurrent programs that
ignore staging can still introduce a transient foreign pairing. Atomic
no-replacement operations detect collisions and trigger safe recovery.

If publication fails, rollback moves the same open payload back to the original
path without replacement. If that fails (for example, another process recreated
the original filename), both payload and complete staged metadata remain in the
SID root. The error names both recovery paths. After closing programs that hold
conflicting handles, move that specific payload to the original path if vacant,
or a separate safe destination. Retain the staged record until recovery is
confirmed. Never remove the payload as cleanup. Pre-move failures clean up only
our own staged metadata; failed cleanup is reported with its location.

This is **not a two-file atomic transaction or a crash-durable journal**. Process
termination, power loss, external deletion, and filesystem/filter-driver faults
may leave a partial stage, an orphan payload plus stage, or a committed pair even
if no success was printed. Only the final metadata rename counts as a successful
commit in a running process. Do not blindly retry a missing source: inspect the
reported paths and listing diagnostics. A complete stage retains the original
path, size, and deletion time; partial stages must not be treated as valid
metadata. Never rename a stage to `$I` until its complete record and matching
payload have been validated. Listing diagnoses orphan `$R` entries and ignores
staging files. No automatic crash recovery or repair command is implemented.

## Explorer compatibility and manual Windows verification

No live Windows tests have been performed. These are separate, **unverified**
claims: structural validity on disk, visibility in Explorer, correct original
path/time display, restoration through Explorer, and persistence across Explorer
restart or reboot. No Shell notification API is called. Explorer may cache its
view; neither refresh behavior nor cache lifetime is established.

Use a disposable Windows 10/11 VM and a fresh test account, with an initialized
Recycle Bin on a local NTFS drive. Keep all checks scoped to the disposable
filenames; never empty the Recycle Bin or delete unrelated entries.

1. Build with `cargo build`. Record Windows version, volume filesystem, test SID,
   local timezone, and the current `wtrash list --json` baseline. If the bin is
   uninitialized, create and recycle a disposable file using Windows Explorer
   manually, then retry. wtrash must never create the SID directory itself.
2. In a fresh test directory create small files with known contents: `plain.txt`,
   `noextension`, `space name.txt`, `日本語.txt`, `-option`, and `list`. Use CMD
   `echo`/redirection or an editor. Record their absolute paths, byte sizes and
   SHA-256 hashes (`certutil -hashfile <path> SHA256`).
3. Run `wtrash trash --dry-run plain.txt "space name.txt"`. Verify all inputs
   remain byte-identical and that no new stage or pair appeared in the SID root.
4. Run `wtrash trash plain.txt "space name.txt"`, then shorthand
   `wtrash noextension 日本語.txt`, and `wtrash -- -option list`. Include one
   absolute input path. Confirm each source disappears, each invocation returns
   0, and exactly one new pair per input appears in `wtrash list --json`.
5. For each new pair inspect the exact `$I` and `$R` names under the test SID.
   Decode `$I` with the codec (or inspect a copy in a hex viewer): version 2,
   original size, FILETIME, little-endian UTF-16 count including NUL, absolute
   original path, and final terminator. Hash each `$R` and compare to the input.
   Record this as structural verification separately from Explorer results.
6. Open the Recycle Bin in Explorer. Record immediate visibility, then manually
   refresh and record visibility again. Inspect Original Location, name, size,
   and deletion time (allowing for local-time presentation). Compare with JSON
   raw FILETIME/UTC and the recorded source paths. Do not infer correct display
   from structural correctness alone.
7. Restore one selected disposable file through Explorer. Verify its original
   path and hash and that its exact pair disappears. Leave other test entries,
   restart Explorer, and record display again. Reboot the VM and repeat display,
   path/time inspection, and restoration/hash checks for remaining entries.
8. Verify explicit refusal for a directory, drive root, an input under the test
   SID directory, a symlink/junction ancestor, a hard-linked disposable file,
   a UNC share, and a non-NTFS volume if available. Confirm no source changes.
   Try a mixed list of valid and nonexistent disposable inputs: valid inputs
   should commit, errors should identify their inputs, and exit status is 1.
9. Run the synthetic failure-injection suite separately when tests are allowed:
   `cargo test transaction::tests` and `cargo test --bin wtrash`. These fixtures
   exercise stage creation/partial writes, movement, publication, concurrent
   collisions, retry exhaustion, rollback, and cleanup failures without touching
   any real Recycle Bin. Native Windows rollback/rename failure behavior remains
   a separate integration-validation requirement.

## Restore and permanent deletion safety

```powershell
cargo install --path .
wtrash list --json
wtrash restore --dry-run 'wtrash1:<volume-guid>:<filename-utf16-hex>'
wtrash restore --create-parents 'wtrash1:<volume-guid>:<filename-utf16-hex>'
wtrash remove --dry-run 'wtrash1:<volume-guid>:<filename-utf16-hex>'
wtrash remove --yes 'wtrash1:<volume-guid>:<filename-utf16-hex>'
wtrash empty --dry-run
# Only in an exclusively disposable account after reviewing the dry run:
wtrash empty --yes
```

Build/install on Windows with Rust 1.85+ and an appropriate Windows linker/toolchain.
Linux and macOS support codec/fixture development only; they cannot manipulate a
Windows Recycle Bin. There is no dependency on Shell recycling APIs or utilities.

Restore validates the decoded absolute DOS path, rejects device names, streams,
ambiguous components, Recycle Bin destinations and reparse-point ancestors, and
pins existing parents. An existing file, directory, or link at the destination
causes failure; the final handle-based rename also atomically refuses replacement.
The original drive must still identify the entry's volume GUID. A changed drive
letter, mounted-folder ancestor, or unavailable volume fails safely; there is no
cross-volume copy fallback. `--create-parents` creates ordinary missing directories
one component at a time and pins them before proceeding. Created empty parents
may remain after a later failure; their original ACLs/timestamps cannot be recovered.
Dry runs create no directories. Moves retain ordinary same-volume file/directory
contents and metadata. Directory restore moves the whole `$R` directory without
traversing or following descendants. Metadata is removed only after a successful
payload move. If cleanup fails, the diagnostic distinguishes already-restored data
from remaining metadata; inspect the reported paths before retrying.

`remove` and `empty` require typing `yes` at a terminal or supplying `--yes`.
Non-interactive calls require `--yes`, except dry runs.
Confirmation covers a snapshot of validated entries, never entries arriving later.
Repeated IDs are deduplicated. Invalid IDs fail individually; other selected entries
are still processed. Emptying reports discovery warnings and returns failure when
its view may be incomplete, even if all selected deletions succeeded.

Only valid current-user `$I`/`$R` pairs are eligible. Exact metadata is re-read
under a handle that denies concurrent writes/deletion and compared to the selection.
Payloads are opened without following reparse points and held against replacement;
multiply linked files are rejected. IDs remain name-based locators: replacement by
an identical record before locking cannot be detected as an identity change.
Neither `$Recycle.Bin` nor the SID directory is ever recursively removed.
Orphans, malformed records, staging files, and unknown files are left untouched.

Directory payload deletion first opens and validates the entire tree, rejecting
all reparse points and multiply linked files before deleting anything. Preflight
is bounded to 128 descendant levels and 16,384 objects per entry. It retains every
handle until deletion or failure. Children are deleted through their validated
handles before parents. New children, access denial, read-only files, or other
errors may still cause partial deletion; the diagnostic says so and retains `$I`
metadata. Failed metadata cleanup after payload deletion is reported separately.
There is no rollback for permanent deletion. A process crash can also leave a
partial tree or an orphan `$I`; no automatic repair is attempted.

**Permanent filesystem deletion does not guarantee physical data sanitization.**
Back up important data. Review IDs and dry-run output before destructive commands.
Directory trashing remains disabled: no actual Windows-generated directory fixture
has been verified for the original-size field and `$R` representation. The program
will not guess directory metadata. Existing directory pairs can be listed, restored,
or removed subject to the checks above. Quotas, eviction, ACL changes, automatic
recovery, and demonstrated Explorer compatibility remain unsupported.

## Additional manual acceptance checks

All live Windows checks require explicit operator opt-in on disposable Windows
10/11 VMs and files. No automated fixture touches the real Recycle Bin. Record
Windows 10 and 11 results independently; cross-compilation proves neither runtime
behavior nor Explorer compatibility.

1. Capture the JSON baseline. Trash the same disposable Unicode/space/extensionless
   filename twice with different known contents; restore each ID independently and
   compare hashes. Repeat with a long DOS path and on a second NTFS volume.
2. Create a destination conflict: restore must fail and retain both pair members.
   Delete original parents: normal restore must fail, dry-run with `--create-parents`
   must create nothing, and real restore with that flag must recover the hash.
3. Change the original drive mapping or insert a junction/mount-point ancestor.
   Restore must refuse without moving the payload. Test inaccessible parents and
   sharing conflicts too; release the conflict and confirm a retry succeeds.
4. On disposable pairs only, confirm remove dry-run changes nothing, cancelling
   the prompt changes nothing, and redirected input without `--yes` fails. Confirm
   explicit deletion removes only the selected pair and returns the proper status.
5. In an exclusively disposable account, seed malformed/truncated records and
   orphan `$I`/`$R` files. Review `empty --dry-run`, then explicitly run `empty --yes`.
   Validated pairs alone should disappear; uncertain files must remain with warnings.
6. Create a disposable directory entry using Explorer, record raw `$I` original size
   and `$R` layout before considering future directory trash support. Exercise wtrash
   restore and remove of this existing directory with nested contents. A nested
   junction must cause removal preflight to fail without touching its target.
7. Exercise concurrent attempts, destination races, access-denied descendants, and
   interrupted restore/remove in the VM. Inspect diagnostics and retained metadata
   and payloads after partial failures. Do not infer crash atomicity from success.
8. Repeat Explorer visibility, path/time display, Explorer restoration, restart,
   and reboot checks from the earlier checklist. Record each result separately.

## Development

Requires Rust 1.85 or later. The library and fixture sources can be compiled on
non-Windows hosts without access to a Recycle Bin.

```sh
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
# With an installed Windows target/toolchain:
cargo check --all-targets --target x86_64-pc-windows-gnu
# Or use x86_64-pc-windows-msvc with its installed target.
```

Codec fixtures cover v1/v2, malformed input, Unicode paths, FILETIME boundaries,
and filename pairing. Listing fixtures cover multiple volumes, repeated paths,
identical entry names, Unicode and native non-Unicode names (Windows), orphans,
corrupt/oversized records, empty/missing/unreadable roots, Unix permission denial
(where enforced), rejected symlinks, safe ID parsing/lookup, deterministic JSON,
64-bit precision, UTC output, and human control-character escaping.
Transaction and CLI fixtures are also defined. Mutation fixtures cover unsafe
metadata destinations, native no-overwrite moves, missing-parent dry runs,
concurrent selection, deletion limits, and injected deletion-access failure in
private temporary directories. Native mutation fixtures compile only on Windows. Tests are **not executed, as
requested**. `cargo check --all-targets` compiles test sources without running them.

## References

The format implementation follows the field layouts in
[libyal's working format specification](https://github.com/libyal/dtformats/blob/main/documentation/Windows%20Recycle.Bin%20file%20formats.asciidoc)
and [recyclebinunit's format notes](https://github.com/danielmarschall/recyclebinunit/blob/master/FORMAT.md).
The latter explicitly describes the v1 fixed buffer and v2 terminator-inclusive
length. [gio(1)](https://man.archlinux.org/man/gio.1) supplies CLI inspiration;
its platform backends and URI behavior are not part of this implementation.

Native discovery follows Microsoft's documentation for
[volume enumeration](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstvolumew),
[drive types](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew),
[token information](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation),
and [SID conversion](https://learn.microsoft.com/en-us/windows/win32/api/sddl/nf-sddl-convertsidtostringsidw).

Trashing uses Microsoft's [handle-based rename structure](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
with replacement disabled and [system-preferred random generation](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptgenrandom).
These API contracts and the format references do not establish Explorer compatibility.
