use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Native Windows Recycle Bin CLI",
    long_about = "Trash, list, restore, and permanently remove current-user Recycle Bin entries on local NTFS volumes.",
    arg_required_else_help = true,
    args_conflicts_with_subcommands = true,
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Shorthand for `trash`. Use -- before option-like paths.
    #[arg(value_name = "PATH", num_args = 1..)]
    paths: Vec<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Move ordinary files to the Recycle Bin.
    Trash {
        /// Validate inputs and show destinations without changing files.
        #[arg(long)]
        dry_run: bool,
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
    },
    /// List the current user's entries on supported local NTFS volumes.
    List {
        /// Emit a versioned JSON document, including diagnostics.
        #[arg(long)]
        json: bool,
    },
    /// Restore entries without overwriting existing destinations.
    Restore {
        #[arg(long)]
        create_parents: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(required = true, num_args = 1..)]
        entry_ids: Vec<OsString>,
    },
    /// Permanently delete selected entries (does not sanitize physical media).
    Remove {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(required = true, num_args = 1..)]
        entry_ids: Vec<OsString>,
    },
    /// Permanently delete all validated current-user entries.
    Empty {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(Command::List { json }) = &cli.command {
        return list(*json);
    }
    match cli.command {
        Some(Command::Trash { paths, dry_run }) => trash(paths, dry_run),
        None => trash(cli.paths, false),
        Some(Command::Restore {
            entry_ids,
            create_parents,
            dry_run,
        }) => mutate(entry_ids, Some(create_parents), false, dry_run, false),
        Some(Command::Remove {
            entry_ids,
            yes,
            dry_run,
        }) => mutate(entry_ids, None, yes, dry_run, false),
        Some(Command::Empty { yes, dry_run }) => mutate(Vec::new(), None, yes, dry_run, true),
        Some(Command::List { .. }) => unreachable!("handled above"),
    }
}

fn confirm(count: usize, yes: bool, dry_run: bool) -> io::Result<()> {
    if yes || dry_run {
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(io::Error::other(
            "permanent deletion requires --yes in non-interactive environments",
        ));
    }
    eprint!("Permanently delete {count} selected entries? Type yes: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim() != "yes" {
        return Err(io::Error::other("permanent deletion cancelled"));
    }
    Ok(())
}

fn mutate(
    ids: Vec<OsString>,
    restore: Option<bool>,
    yes: bool,
    dry_run: bool,
    empty: bool,
) -> ExitCode {
    let listing = match wtrash::platform::list() {
        Ok(listing) => listing,
        Err(error) => {
            eprintln!("wtrash: {error}");
            return ExitCode::FAILURE;
        }
    };
    for diagnostic in &listing.diagnostics {
        eprintln!(
            "wtrash: {}: {}: {}",
            diagnostic.code, diagnostic.location, diagnostic.message
        );
    }
    // Snapshot the selection before prompting; newly appearing entries are never included.
    let mut selected = Vec::new();
    let mut failed = !listing.diagnostics.is_empty() && empty;
    if empty {
        selected.extend(listing.entries.iter());
    } else {
        for raw in ids {
            let id = raw
                .to_str()
                .and_then(|s| s.parse::<wtrash::listing::EntryId>().ok());
            match id.as_ref().and_then(|id| listing.find(id)) {
                Some(entry)
                    if !selected
                        .iter()
                        .any(|e: &&wtrash::listing::Entry| e.id == entry.id) =>
                {
                    selected.push(entry)
                }
                Some(_) => {}
                None => {
                    eprintln!(
                        "wtrash: {raw:?}: invalid ID or entry not found in the current user's bin"
                    );
                    failed = true;
                }
            }
        }
    }
    if restore.is_none() {
        if let Err(error) = confirm(selected.len(), yes, dry_run) {
            eprintln!("wtrash: {error}");
            return ExitCode::FAILURE;
        }
    }
    for entry in selected {
        match wtrash::platform::mutate(entry, restore, dry_run) {
            Ok(()) => println!(
                "{} {}",
                match (dry_run, restore.is_some()) {
                    (true, true) => "would restore",
                    (true, false) => "would permanently remove",
                    (false, true) => "restored",
                    (false, false) => "permanently removed",
                },
                entry.id
            ),
            Err(error) => {
                eprintln!("wtrash: {}: {error}", entry.id);
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn trash(paths: Vec<PathBuf>, dry_run: bool) -> ExitCode {
    let mut failed = false;
    for path in paths {
        match wtrash::platform::trash(&path, dry_run) {
            Ok(destination) => println!(
                "{} {:?} -> {:?}",
                if dry_run { "would trash" } else { "trashed" },
                path,
                destination
            ),
            Err(error) => {
                eprintln!("wtrash: {path:?}: {error}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn list(json: bool) -> ExitCode {
    let listing = match wtrash::platform::list() {
        Ok(listing) => listing,
        Err(error) => {
            eprintln!("wtrash: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !json {
        for diagnostic in &listing.diagnostics {
            eprintln!(
                "wtrash: {}: {}: {}",
                diagnostic.code, diagnostic.location, diagnostic.message
            );
        }
    }
    let stdout = std::io::stdout();
    let result = if json {
        wtrash::presentation::write_json(&listing, stdout.lock())
    } else {
        wtrash::presentation::write_human(&listing, stdout.lock())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wtrash: write listing: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_options_and_required_ids() {
        assert!(matches!(
            Cli::try_parse_from(["wtrash", "restore", "--create-parents", "--dry-run", "id"])
                .unwrap()
                .command,
            Some(Command::Restore {
                create_parents: true,
                dry_run: true,
                ..
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["wtrash", "remove", "--yes", "--dry-run", "id"])
                .unwrap()
                .command,
            Some(Command::Remove {
                yes: true,
                dry_run: true,
                ..
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["wtrash", "empty", "--yes"])
                .unwrap()
                .command,
            Some(Command::Empty {
                yes: true,
                dry_run: false
            })
        ));
        assert!(Cli::try_parse_from(["wtrash", "restore"]).is_err());
        assert!(Cli::try_parse_from(["wtrash", "remove", "--yes"]).is_err());
        assert!(confirm(1, true, false).is_ok());
        assert!(confirm(1, false, true).is_ok());
    }

    #[test]
    fn paths_subcommands_and_separator() {
        let cli =
            Cli::try_parse_from(["wtrash", "trash", "--dry-run", "--", "-file", "日本語 file"])
                .unwrap();
        assert!(
            matches!(cli.command, Some(Command::Trash { dry_run: true, paths }) if paths == [PathBuf::from("-file"), PathBuf::from("日本語 file")])
        );
        let cli = Cli::try_parse_from(["wtrash", "--", "list", "-file"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.paths, [PathBuf::from("list"), PathBuf::from("-file")]);
        assert!(matches!(
            Cli::try_parse_from(["wtrash", "list", "--json"])
                .unwrap()
                .command,
            Some(Command::List { json: true })
        ));
        assert!(Cli::try_parse_from(["wtrash", "trash", "--dry-run"]).is_err());
    }
}
