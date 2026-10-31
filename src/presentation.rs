//! CLI formatting, separate from discovery and filesystem enumeration.
use crate::listing::{display_path, EntryKind, Listing};
use crate::model::FileTime;
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde::Serialize;
use std::io::{self, Write};

fn utc(time: FileTime) -> Option<String> {
    let ticks = i128::from(time.0) - i128::from(FileTime::UNIX_EPOCH_TICKS);
    let seconds = i64::try_from(ticks.div_euclid(10_000_000)).ok()?;
    let nanos = (ticks.rem_euclid(10_000_000) * 100) as u32;
    let datetime = DateTime::<Utc>::from_timestamp(seconds, nanos)?;
    if !(0..=9999).contains(&datetime.year()) {
        return None;
    }
    Some(datetime.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

#[derive(Serialize)]
struct JsonEntry<'a> {
    id: String,
    volume: String,
    entry_name_utf16: &'a [u16],
    original_path: String,
    original_path_utf16: &'a [u16],
    original_size: String,
    deleted_at_filetime: String,
    deleted_at_utc: Option<String>,
    kind: &'static str,
}

fn kind(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Directory => "directory",
    }
}

pub fn write_json(listing: &Listing, mut output: impl Write) -> io::Result<()> {
    #[derive(Serialize)]
    struct Document<'a> {
        schema_version: u32,
        entries: Vec<JsonEntry<'a>>,
        diagnostics: &'a [crate::listing::Diagnostic],
    }
    let entries = listing
        .entries
        .iter()
        .map(|entry| JsonEntry {
            id: entry.id.to_string(),
            volume: entry.volume.to_string(),
            entry_name_utf16: entry.id.filename_utf16(),
            original_path: String::from_utf16(entry.metadata.original_path.as_utf16())
                .expect("WindowsPath guarantees valid UTF-16"),
            original_path_utf16: entry.metadata.original_path.as_utf16(),
            original_size: entry.metadata.original_size.to_string(),
            deleted_at_filetime: entry.metadata.deleted_at.0.to_string(),
            deleted_at_utc: utc(entry.metadata.deleted_at),
            kind: kind(entry.kind),
        })
        .collect();
    serde_json::to_writer_pretty(
        &mut output,
        &Document {
            schema_version: 1,
            entries,
            diagnostics: &listing.diagnostics,
        },
    )?;
    writeln!(output)
}

pub fn write_human(listing: &Listing, mut output: impl Write) -> io::Result<()> {
    writeln!(
        output,
        "ID\tORIGINAL PATH\tDELETED (UTC)\tSIZE (BYTES)\tTYPE\tVOLUME"
    )?;
    for entry in &listing.entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}",
            entry.id,
            display_path(entry.metadata.original_path.as_utf16()),
            utc(entry.metadata.deleted_at)
                .unwrap_or_else(|| format!("FILETIME({})", entry.metadata.deleted_at.0)),
            entry.metadata.original_size,
            kind(entry.kind),
            entry.volume
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_preserves_pre_epoch_ticks_and_rejects_dates_outside_rfc3339() {
        assert_eq!(
            utc(FileTime(FileTime::UNIX_EPOCH_TICKS - 1)).as_deref(),
            Some("1969-12-31T23:59:59.999999900Z")
        );
        assert_eq!(
            utc(FileTime(0)).as_deref(),
            Some("1601-01-01T00:00:00.000000000Z")
        );
        assert_eq!(utc(FileTime(u64::MAX)), None);
    }
}
