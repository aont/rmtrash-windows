# wtrash

A native Windows 10/11 Recycle Bin CLI. Stage 2 implements **read-only listing**
using ordinary Windows token, volume, and filesystem APIs. It never creates,
moves, restores, deletes, or changes permissions in the actual Recycle Bin.
Only the current process user's entries on local fixed/removable NTFS volumes
are considered. Other commands remain unavailable.

The on-disk Recycle Bin layout is an undocumented Windows implementation detail,
not a guaranteed public interface. Encoding a record is not sufficient to recycle
a file safely; mutating filesystem operations are deliberately deferred.

## CLI

```text
wtrash trash <PATH>...
wtrash list [--json]
wtrash restore <ENTRY_ID>...
wtrash remove <ENTRY_ID>...
wtrash empty
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
hosts, listing fails with an explicit Windows-required error. Mutation commands
return status 1 without accessing the Recycle Bin.

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
`Listing::find` compares a parsed ID against freshly enumerated entries. No ID
is joined to a filesystem root or interpreted as an arbitrary path. Future
mutating stages must rediscover the current SID's roots, match exact entry names,
and revalidate ownership, handles, and entry contents before acting. IDs are
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
- `src/presentation.rs`: human and versioned JSON serialization.
- `src/codec/tests.rs`, `src/listing/tests.rs`: codec and synthetic filesystem fixtures.

No Shell/COM Recycle Bin interface, shell-backed recycling crate, PowerShell,
Explorer, or external trash utility is used. Tests can supply `RecycleRoot`
values directly to `listing::enumerate`; there is deliberately no CLI switch or
environment variable redirecting production discovery to an arbitrary user root.

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

## Unsupported in this stage

Trashing, restoring, permanent removal, emptying, permission/ownership changes,
cross-volume moves, collision-safe allocation, rollback/crash recovery, and
mutating reparse-point/hard-link policies. No live Windows Recycle Bin integration
validation has been performed.

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
Tests are defined but **not executed, as requested**. `cargo check --all-targets`
compiles test sources without running them.

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
