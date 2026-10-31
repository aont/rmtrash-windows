# wtrash

A Rust foundation for a native Windows 10/11 Recycle Bin CLI. Stage 1 implements
an in-memory metadata codec and command parsing only. **No command accesses or
modifies the Recycle Bin.** The intended filesystem scope is local NTFS volumes.

The on-disk Recycle Bin layout is an undocumented Windows implementation detail,
not a guaranteed public interface. Encoding a record is not sufficient to recycle
a file safely; native filesystem operations are deliberately deferred.

## CLI

```text
wtrash trash <PATH>...
wtrash list
wtrash restore <ENTRY_ID>...
wtrash remove <ENTRY_ID>...
wtrash empty
wtrash <PATH>...                 # shorthand for trash
```

Help and version output are available through `--help` and `--version`. All
operation commands return exit status 1 with an explicit unavailable error:
on Windows they report that the operation is not implemented, and on other
systems they report that Windows is required. Invalid arguments return status 2.
Arguments use `PathBuf` or `OsString`; no lossy UTF-8 conversion is used. A path
matching a command name must be qualified (for example, `.\list`); prefix
option-like paths with `--`. There is no success placeholder for listing or
modifying entries.

## Modules

- `src/main.rs`: clap argument parsing and error/exit handling.
- `src/model.rs`: metadata, raw FILETIME, validated Windows paths, filename pairs.
- `src/codec.rs`: bounded byte-slice decoding and version 2 encoding, without I/O.
- `src/platform/`: unavailable-operation gate and the future Windows filesystem
  backend boundary. It currently performs no filesystem or native API calls.
- `src/codec/tests.rs`: independent fixtures and malformed-input cases, including
  conditional Windows native-path coverage.

No Shell/COM Recycle Bin interface, shell-backed recycling crate, PowerShell,
Explorer, or external trash utility is used.

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

Actual trashing, listing, restoring, permanent removal, and emptying; volume and
SID discovery; NTFS verification; permissions and ownership handling; cross-volume
moves; collision-safe ID generation; rollback and crash recovery; reparse-point
and hard-link policy; non-NTFS, network, and legacy INFO/INFO2 layouts. No real
Recycle Bin compatibility or integration validation has been performed.

## Development

Requires Rust 1.85 or later. The library and fixture sources can be compiled on
non-Windows hosts without access to a Recycle Bin.

```sh
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
# With an installed Windows target/toolchain:
cargo check --all-targets --target x86_64-pc-windows-msvc
```

Fixtures cover v1/v2, exact encoder bytes, surrogate pairs and Unicode paths,
all truncated prefixes, unknown versions, missing/embedded NUL, invalid lengths,
oversized inputs, malformed UTF-16, maximum-length paths, v1 padding, FILETIME
boundaries, and variable-length filename pairing. Tests are defined but were
**not executed for this implementation, as requested**.

## References

The format implementation follows the field layouts in
[libyal's working format specification](https://github.com/libyal/dtformats/blob/main/documentation/Windows%20Recycle.Bin%20file%20formats.asciidoc)
and [recyclebinunit's format notes](https://github.com/danielmarschall/recyclebinunit/blob/master/FORMAT.md).
The latter explicitly describes the v1 fixed buffer and v2 terminator-inclusive
length. [gio(1)](https://man.archlinux.org/man/gio.1) supplies CLI inspiration;
its platform backends and URI behavior are not part of this implementation.
