use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Native Windows Recycle Bin CLI",
    long_about = "Trash ordinary files and list the current process user's Recycle Bin on local NTFS volumes.",
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
    /// Restore entries (not implemented).
    Restore {
        #[arg(required = true, num_args = 1..)]
        entry_ids: Vec<OsString>,
    },
    /// Permanently remove entries (not implemented).
    Remove {
        #[arg(required = true, num_args = 1..)]
        entry_ids: Vec<OsString>,
    },
    /// Empty the Recycle Bin (not implemented).
    Empty,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(Command::List { json }) = &cli.command {
        return list(*json);
    }
    match cli.command {
        Some(Command::Trash { paths, dry_run }) => trash(paths, dry_run),
        None => trash(cli.paths, false),
        Some(command) => {
            let operation = match command {
                Command::Restore { .. } => "restore",
                Command::Remove { .. } => "remove",
                Command::Empty => "empty",
                _ => unreachable!("handled above"),
            };
            eprintln!("wtrash: {}", wtrash::platform::unavailable(operation));
            ExitCode::FAILURE
        }
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
