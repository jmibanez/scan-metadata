use clap::Parser;
use log::{LevelFilter, error};
use scan_metadata::camera_profiles::read_camera_profiles_fallback_to_prefs;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use thiserror::Error;

use std::env::current_dir;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scan_metadata::models::{
    MetadataEntry, MetadataReadError, dayone_export_zip_to_json, to_metadata_entries,
};
use scan_metadata::{cli_message, util};

#[derive(Parser)]
struct Args {
    /// Quiet; minimize output to errors
    #[arg(short, long)]
    quiet: bool,

    /// Turn on debug logging
    #[arg(long, conflicts_with = "quiet")]
    debug: bool,

    /// Use YAML with camera and lens profiles
    #[arg(short, long)]
    profiles: Option<PathBuf>,

    /// Directory to output per-roll ZIPs to (default, same directory as source export)
    #[arg(short, long)]
    output_directory: Option<PathBuf>,

    /// The path to the exported metadata, as a ZIP file
    dayone_export_zip: PathBuf,
}

#[derive(Error, Debug)]
pub enum ProgramError {
    #[error("Invalid Day One export ZIP file")]
    InvalidZipFile(#[from] MetadataReadError),

    #[error("No current working directory")]
    NoCurrentWorkingDirectoryError(#[from] std::io::Error),
}

fn split_metadata_entries_into_rolls(
    metadata_entries: Vec<MetadataEntry>,
    output_directory: &Path,
) -> (u32, u32) {
    todo!()
}

fn split_rolls() -> Result<(), ProgramError> {
    let args = Args::parse();

    if args.quiet {
        util::set_log_level(LevelFilter::Off);
    } else if args.debug {
        util::set_log_level(LevelFilter::Debug);
    }
    let level = util::get_log_level();

    let _ = TermLogger::init(
        level,
        Config::default(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    );

    let mut basedir = args.dayone_export_zip.clone();
    let fallback_dir = if !basedir.pop() {
        current_dir()?
    } else {
        basedir
    };

    let json = dayone_export_zip_to_json(args.dayone_export_zip)?;
    let camera_profiles = read_camera_profiles_fallback_to_prefs(args.profiles)?;

    let metadata_entries: Vec<MetadataEntry> = to_metadata_entries(json, camera_profiles);

    let output_directory = if let Some(specified_dir) = args.output_directory {
        specified_dir
    } else {
        fallback_dir
    };
    let (process_count, entry_count) =
        split_metadata_entries_into_rolls(metadata_entries, &output_directory);

    cli_message!(
        "Split into {} separate metadata roll(s), scanned {} entries",
        process_count,
        entry_count
    );
    Ok(())
}

fn main() -> ExitCode {
    let result = split_rolls();
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(ProgramError::InvalidZipFile(e)) => {
            error!("Invalid metadata: {}", e);
            ExitCode::from(2)
        }
        Err(ProgramError::NoCurrentWorkingDirectoryError(e)) => {
            error!("No current working directory: {}", e);
            ExitCode::from(2)
        }
    }
}
