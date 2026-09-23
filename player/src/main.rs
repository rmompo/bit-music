mod commands;
mod help;
mod keyboard;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// bit-music composition player.
///
/// `version` and `help` are only available as subcommands (`bm version`,
/// `bm help`) — no `--version`/`--help`/`-V`/`-v`/`-h` at the top level, to
/// avoid two different ways of asking for the same thing.
#[derive(Parser)]
#[command(
    name = "bm",
    about = "bit-music composition player",
    disable_help_subcommand = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Play a composition
    Play {
        /// Path to the .bm1 file
        file: PathBuf,
        /// Loop playback continuously until interrupted with Ctrl+C or Escape
        #[arg(long = "non-stop")]
        non_stop: bool,
    },
    /// Check the structural integrity of a composition file
    CheckIntegrity {
        /// Path to the .bm1 file
        file: PathBuf,
    },
    /// Check that every referenced sample file is available
    CheckSamples {
        /// Path to the .bm1 file
        file: PathBuf,
    },
    /// Run both check-integrity and check-samples
    Check {
        /// Path to the .bm1 file
        file: PathBuf,
    },
    /// Export a composition to an audio file (same base name as the input,
    /// in the same directory)
    Export {
        /// Path to the .bm1 file
        file: PathBuf,
        /// Export to .wav (currently the only supported format)
        #[arg(long)]
        wav: bool,
    },
    /// Print the bm version
    Version,
    /// Show help (loaded from bm.hlp, editable without recompiling)
    Help,
    /// (Placeholder) automatically edits/improves a composition, writing
    /// the result to a new file rather than modifying the original —
    /// switches are not defined yet
    Magik {
        /// Path to the .bm1 file
        file: PathBuf,
        /// Reserved for future switches (not implemented yet)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        switches: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let ok = match cli.command {
        Command::Play { file, non_stop } => commands::play(&file, non_stop),
        Command::CheckIntegrity { file } => commands::check_integrity(&file),
        Command::CheckSamples { file } => commands::check_samples(&file),
        Command::Check { file } => {
            let integrity_ok = commands::check_integrity(&file);
            let samples_ok = commands::check_samples(&file);
            integrity_ok && samples_ok
        }
        Command::Export { file, wav } => commands::export(&file, wav),
        Command::Version => {
            println!("bm {}", env!("CARGO_PKG_VERSION"));
            println!(
                "supported .bm1 format version(s): {}",
                bm_format::SUPPORTED_FORMAT_VERSIONS.join(", ")
            );
            true
        }
        Command::Help => {
            help::print_help();
            true
        }
        Command::Magik { .. } => {
            println!("to be implemented");
            true
        }
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
