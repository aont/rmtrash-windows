use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Native Windows Recycle Bin CLI (foundation only)",
    long_about = "Native Windows Recycle Bin CLI foundation. All Recycle Bin operations are currently unavailable and return failure without filesystem access.",
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
    /// List entries (not implemented).
    List,
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
    let operation = match cli.command {
        Some(Command::Trash { .. }) | None => "trash",
        Some(Command::List) => "list",
        Some(Command::Restore { .. }) => "restore",
        Some(Command::Remove { .. }) => "remove",
        Some(Command::Empty) => "empty",
    };
    eprintln!("wtrash: {}", wtrash::platform::unavailable(operation));
    ExitCode::FAILURE
}
