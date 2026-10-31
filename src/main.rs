use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Native Windows Recycle Bin CLI (read-only listing)",
    long_about = "List the current process user's Recycle Bin on local NTFS volumes. Commands that modify the Recycle Bin remain unavailable.",
    arg_required_else_help = true,
    args_conflicts_with_subcommands = true,
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Shorthand for `trash` (currently unavailable). Use -- before option-like paths.
    #[arg(value_name = "PATH", num_args = 1..)]
    paths: Vec<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Move paths to the Recycle Bin (not implemented).
    Trash {
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
    let operation = match cli.command {
        Some(Command::Trash { .. }) | None => "trash",
        Some(Command::List { .. }) => unreachable!("listing handled above"),
        Some(Command::Restore { .. }) => "restore",
        Some(Command::Remove { .. }) => "remove",
        Some(Command::Empty) => "empty",
    };
    eprintln!("wtrash: {}", wtrash::platform::unavailable(operation));
    ExitCode::FAILURE
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
