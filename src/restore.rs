//! Conservative validation before interpreting metadata paths as DOS destinations.
use std::io;

pub(crate) fn validate_destination(path: &[u16]) -> io::Result<()> {
    let invalid = || {
        io::Error::other(
            "unsafe original path: expected an absolute local DOS path with ordinary components",
        )
    };
    let text = String::from_utf16(path).map_err(|_| invalid())?;
    let bytes = text.as_bytes();
    if bytes.len() < 4 || !bytes[0].is_ascii_alphabetic() || &bytes[1..3] != b":\\" {
        return Err(invalid());
    }
    for component in text[3..].split('\\') {
        let base = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
            || ["COM", "LPT"].iter().any(|prefix| {
                base.strip_prefix(prefix).is_some_and(|n| {
                    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&n)
                })
            });
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component
                .chars()
                .any(|c| c.is_control() || "<>:\"/|?*".contains(c))
            || component.eq_ignore_ascii_case("$Recycle.Bin")
            || component.encode_utf16().count() > 255
            || device
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_safety() {
        for path in [
            r"C:\日本語\space name",
            r"D:\noextension",
            &format!("C:\\{}file.txt", "a\\".repeat(150)),
        ] {
            assert!(
                validate_destination(&path.encode_utf16().collect::<Vec<_>>()).is_ok(),
                "{path}"
            );
        }
        for path in [
            r"C:\",
            r"C:relative",
            r"\rooted",
            r"\\server\share\file",
            r"\\?\C:\file",
            r"C:\a\..\b",
            r"C:\a\.\b",
            r"C:\a\\b",
            r"C:\file:stream",
            r"C:\a.\file",
            r"C:\a \file",
            r"C:\$Recycle.Bin\file",
            r"C:\CON.txt",
            r"C:\LPT¹",
            "C:\\bad\nname",
        ] {
            assert!(
                validate_destination(&path.encode_utf16().collect::<Vec<_>>()).is_err(),
                "{path}"
            );
        }
        assert!(validate_destination(&[67, 58, 92, 0xd800]).is_err());
    }
}
