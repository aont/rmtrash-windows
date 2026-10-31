use crate::codec::{decode, encode_v2, CodecError, MAX_RECORD_BYTES};
use crate::model::{
    EntryNames, FileTime, FormatVersion, Metadata, PathError, WindowsPath, MAX_PATH_UNITS,
};
use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

// Independently specified header: v2, size 0x0102030405060708, Unix epoch FILETIME.
const HEADER: [u8; 24] = [
    2, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0, 128, 62, 213, 222, 177, 157, 1,
];

// Fixture builder does not call the encoder or the domain path conversions.
fn v2(units: &[u16]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn v1() -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    bytes[0] = 1;
    // C:\a followed by NUL and fixed-buffer padding.
    bytes.extend_from_slice(&[67, 0, 58, 0, 92, 0, 97, 0, 0, 0]);
    bytes.resize(544, 0);
    bytes
}

#[test]
fn independent_v1_fixture() {
    let record = decode(&v1()).unwrap();
    assert_eq!(record.version, FormatVersion::V1);
    assert_eq!(record.original_size, 0x0102030405060708);
    assert_eq!(record.deleted_at, FileTime(FileTime::UNIX_EPOCH_TICKS));
    assert_eq!(record.original_path.as_utf16(), &[67, 58, 92, 97]);
    assert_eq!(
        record.original_path.to_path_buf().unwrap(),
        Path::new("C:\\a")
    );
    assert_eq!(encode_v2(&record).unwrap(), v2(&[67, 58, 92, 97, 0]));
}

#[test]
fn independent_unicode_v2_fixture_and_exact_encoding() {
    // C:\猫😀 : surrogate pair counts as two code units, with a final NUL.
    let units = [67, 58, 92, 0x732b, 0xd83d, 0xde00, 0];
    let bytes = v2(&units);
    let record = decode(&bytes).unwrap();
    assert_eq!(record.version, FormatVersion::V2);
    assert_eq!(record.original_path.as_utf16(), &units[..6]);
    assert_eq!(
        record.original_path.to_path_buf().unwrap(),
        Path::new("C:\\猫😀")
    );
    assert_eq!(encode_v2(&record).unwrap(), bytes);
    assert_eq!(bytes.len(), 28 + 7 * 2);
    assert_eq!(&bytes[24..28], &[7, 0, 0, 0]);
}

#[test]
fn independent_encoder_input() {
    let record = Metadata {
        version: FormatVersion::V1,
        original_size: 0x0102030405060708,
        deleted_at: FileTime(FileTime::UNIX_EPOCH_TICKS),
        original_path: WindowsPath::from_path(Path::new("C:\\a")).unwrap(),
    };
    assert_eq!(encode_v2(&record).unwrap(), v2(&[67, 58, 92, 97, 0]));
}

#[test]
fn all_truncated_prefixes_are_errors() {
    for fixture in [v1(), v2(&[67, 58, 92, 97, 0])] {
        for end in 0..fixture.len() {
            assert!(
                matches!(decode(&fixture[..end]), Err(CodecError::Truncated { .. })),
                "prefix {end}"
            );
        }
    }
}

#[test]
fn invalid_declared_lengths_and_trailing_bytes() {
    for count in [0_u32, 1] {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(&count.to_le_bytes());
        assert_eq!(decode(&bytes), Err(CodecError::InvalidLength(count)));
    }
    for count in [MAX_PATH_UNITS as u32 + 1, u32::MAX] {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(&count.to_le_bytes());
        assert_eq!(decode(&bytes), Err(CodecError::TooLarge));
    }
    for mut bytes in [v1(), v2(&[65, 0])] {
        bytes.push(0);
        assert!(matches!(
            decode(&bytes),
            Err(CodecError::InconsistentLength { .. })
        ));
    }
    let mut short = v2(&[65, 0]);
    short[24..28].copy_from_slice(&3_u32.to_le_bytes());
    assert!(matches!(decode(&short), Err(CodecError::Truncated { .. })));
    assert_eq!(
        decode(&vec![0; MAX_RECORD_BYTES + 1]),
        Err(CodecError::TooLarge)
    );
}

#[test]
fn unknown_version_is_explicit() {
    let mut bytes = v1();
    bytes[..8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        decode(&bytes),
        Err(CodecError::UnsupportedVersion(u64::MAX))
    );
}

#[test]
fn nul_rules_and_malformed_utf16() {
    assert_eq!(decode(&v2(&[65, 66])), Err(CodecError::MissingTerminator));
    assert_eq!(
        decode(&v2(&[65, 0, 66, 0])),
        Err(CodecError::Path(PathError::EmbeddedNul))
    );
    assert_eq!(
        decode(&v2(&[0, 0])),
        Err(CodecError::Path(PathError::EmbeddedNul))
    );
    for units in [&[0xd800, 0][..], &[0xdc00, 0], &[0xd800, 65, 0]] {
        assert_eq!(
            decode(&v2(units)),
            Err(CodecError::Path(PathError::InvalidUtf16))
        );
    }
    let mut bytes = v1();
    bytes[24..26].copy_from_slice(&0xd800_u16.to_le_bytes());
    assert_eq!(
        decode(&bytes),
        Err(CodecError::Path(PathError::InvalidUtf16))
    );
    bytes[24..].fill(65);
    assert_eq!(decode(&bytes), Err(CodecError::MissingTerminator));
    let mut bytes = v1();
    bytes[24..26].fill(0);
    assert_eq!(decode(&bytes), Err(CodecError::Path(PathError::Empty)));
}

#[test]
fn v1_padding_is_ignored_and_full_buffer_path_supported() {
    let mut bytes = v1();
    bytes[34..].fill(0xff);
    assert!(decode(&bytes).is_ok());
    let mut bytes = HEADER.to_vec();
    bytes[0] = 1;
    for _ in 0..259 {
        bytes.extend_from_slice(&[65, 0]);
    }
    bytes.extend_from_slice(&[0, 0]);
    assert_eq!(decode(&bytes).unwrap().original_path.as_utf16().len(), 259);
}

#[test]
fn upper_bound_and_domain_validation() {
    let mut units = vec![65; MAX_PATH_UNITS];
    units[MAX_PATH_UNITS - 1] = 0;
    let bytes = v2(&units);
    let record = decode(&bytes).unwrap();
    assert_eq!(encode_v2(&record).unwrap(), bytes);
    assert_eq!(
        WindowsPath::from_utf16(&vec![65; MAX_PATH_UNITS]),
        Err(PathError::TooLong)
    );
    assert_eq!(WindowsPath::from_utf16(&[]), Err(PathError::Empty));
    assert_eq!(
        WindowsPath::from_utf16(&[65, 0]),
        Err(PathError::EmbeddedNul)
    );
}

#[test]
fn filetime_epoch_subsecond_and_before_epoch() {
    assert_eq!(
        FileTime(FileTime::UNIX_EPOCH_TICKS).to_system_time(),
        Some(UNIX_EPOCH)
    );
    assert_eq!(
        FileTime(FileTime::UNIX_EPOCH_TICKS + 1).to_system_time(),
        UNIX_EPOCH.checked_add(Duration::from_nanos(100))
    );
    assert_eq!(
        FileTime(FileTime::UNIX_EPOCH_TICKS - 1).to_system_time(),
        UNIX_EPOCH.checked_sub(Duration::from_nanos(100))
    );
    assert_eq!(
        FileTime(0).to_system_time(),
        UNIX_EPOCH.checked_sub(Duration::from_secs(11_644_473_600))
    );
    let mut bytes = v2(&[65, 0]);
    bytes[8..24].fill(0xff);
    let record = decode(&bytes).unwrap();
    assert_eq!(record.original_size, u64::MAX);
    assert_eq!(record.deleted_at, FileTime(u64::MAX));
    assert_eq!(encode_v2(&record).unwrap(), bytes);
}

#[test]
fn variable_length_filename_pairing() {
    for suffix in ["x", "ABC123.txt", "much-longer-identifier.tar.gz", "猫😀"] {
        let i = format!("$I{suffix}");
        let r = format!("$R{suffix}");
        let names = EntryNames::from_name(OsStr::new(&i)).unwrap();
        assert_eq!(names.metadata, OsStr::new(&i));
        assert_eq!(names.payload, OsStr::new(&r));
        assert_eq!(EntryNames::from_name(OsStr::new(&r)).unwrap(), names);
    }
    for name in [
        "",
        "$I",
        "$R",
        "$Xabc",
        "dir/$Iabc",
        "$I../x",
        "$I..\\x",
        "$Iabc:ads",
        "$Ia\0b",
    ] {
        assert_eq!(
            EntryNames::from_name(OsStr::new(name)),
            Err(PathError::InvalidPairName)
        );
    }
}

#[cfg(windows)]
#[test]
fn native_windows_path_and_filename_preservation() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let path = WindowsPath::from_utf16(&[67, 58, 92, 0x732b, 0xd83d, 0xde00]).unwrap();
    assert_eq!(
        path.to_path_buf()
            .unwrap()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        path.as_utf16()
    );
    let malformed = std::ffi::OsString::from_wide(&[67, 58, 92, 0xd800]);
    assert_eq!(
        WindowsPath::from_path(Path::new(&malformed)),
        Err(PathError::InvalidUtf16)
    );
    // Pairing is a native filename transformation, including non-Unicode names.
    let name = std::ffi::OsString::from_wide(&[36, 73, 0xd800]);
    assert_eq!(
        EntryNames::from_name(&name)
            .unwrap()
            .payload
            .encode_wide()
            .collect::<Vec<_>>(),
        [36, 82, 0xd800]
    );
}
