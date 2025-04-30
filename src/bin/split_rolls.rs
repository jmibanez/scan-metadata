use clap::Parser;
use itertools::Itertools;
use log::{LevelFilter, debug, error, warn};
use scan_metadata::camera_profiles::read_camera_profiles_fallback_to_prefs;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use thiserror::Error;

use std::collections::HashMap;
use std::env::current_dir;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scan_metadata::models::{
    LeaderEntry, MetadataEntryType, MetadataReadError, Roll, dayone_export_zip_to_json,
    to_metadata_entries,
};
use scan_metadata::{cli_message, util};

/// Split a Day One export ZIP into separate ZIP files, one per roll of film recorded.
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

fn split_metadata_rolls<'a>(metadata_entries: &Vec<&'a MetadataEntryType>) -> Vec<Roll<'a>> {
    let leader = metadata_entries.first().unwrap();
    let camera = leader.entry().camera_tag().unwrap();
    let mut rolls = Vec::new();

    let mut current_leader: Option<&LeaderEntry> = None;
    let mut current_roll: Vec<&MetadataEntryType> = Vec::new();

    for entry in metadata_entries {
        match entry {
            MetadataEntryType::Leader(leader) => {
                if !current_roll.is_empty() {
                    let leader = current_leader.unwrap();
                    let roll = Roll::new(camera.clone(), leader.emulsion(), current_roll.clone());
                    current_roll.clear();
                    rolls.push(roll);
                }
                current_leader = Some(leader);
                current_roll.push(entry);
            }
            MetadataEntryType::Frame(_) => {
                current_roll.push(entry);
            }
        }
    }

    // Slurp final group
    if !current_roll.is_empty() {
        let leader = current_leader.unwrap();
        let roll = Roll::new(camera.clone(), leader.emulsion(), current_roll.clone());
        current_roll.clear();
        rolls.push(roll);
    }

    rolls
}

fn group_entries_by_camera(
    metadata_entries: &[MetadataEntryType],
) -> HashMap<Option<&String>, Vec<&MetadataEntryType>> {
    let camera_roll_groups = metadata_entries
        .iter()
        .into_group_map_by(|e| e.entry().camera_tag());

    debug!("Found {} camera groups", camera_roll_groups.keys().count());

    camera_roll_groups
}

fn split_metadata_entries_into_rolls(
    metadata_entries: &[MetadataEntryType],
    output_directory: &Path,
) -> (usize, usize) {
    let total_count = metadata_entries.len();
    let mut process_count = 0;

    assert!(
        output_directory.is_dir(),
        "output_directory is not a directory!"
    );

    let camera_roll_groups = group_entries_by_camera(metadata_entries);

    for (maybe_camera, entry_list) in camera_roll_groups {
        if maybe_camera.is_none() {
            // Warn about not having an associated camera
            warn!(
                "Found {} entries without an associated camera tag!",
                entry_list.len(),
            );
            debug!("\tEntries that don't have associated camera tag: {entry_list:#?}");
            continue;
        }
        let camera = maybe_camera.unwrap();

        let this_camera_rolls = split_metadata_rolls(&entry_list);
        for (i, roll) in this_camera_rolls.iter().enumerate() {
            debug!(
                "Camera {camera}: Writing roll {i} to {}",
                output_directory.display()
            );
            match roll.serialize_to(i, output_directory) {
                Ok(()) => (),
                Err(e) => {
                    error!(
                        "Could not serialize roll to {}: {e}",
                        output_directory.display()
                    );
                }
            }
            process_count += 1;
        }
    }

    (process_count, total_count)
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
    let fallback_dir = if basedir.pop() && basedir.exists() {
        debug!("Basedir: {}", basedir.display());
        basedir
    } else {
        debug!("Current dir: {}", current_dir()?.display());
        current_dir()?
    };

    let json = dayone_export_zip_to_json(args.dayone_export_zip)?;
    let camera_profiles = read_camera_profiles_fallback_to_prefs(args.profiles)?;

    let metadata_entries = to_metadata_entries(&json, camera_profiles);

    let output_directory = if let Some(specified_dir) = args.output_directory {
        debug!("Using specified directory: {}", specified_dir.display());
        specified_dir
    } else {
        debug!("Using fallback directory: {}", fallback_dir.display());
        fallback_dir
    };
    let (process_count, entry_count) =
        split_metadata_entries_into_rolls(&metadata_entries, &output_directory);

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
        Ok(()) => ExitCode::SUCCESS,
        Err(ProgramError::InvalidZipFile(e)) => {
            error!("Invalid metadata: {e}");
            ExitCode::from(2)
        }
        Err(ProgramError::NoCurrentWorkingDirectoryError(e)) => {
            error!("No current working directory: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Error;

    use chrono::DateTime;
    use scan_metadata::models::FrameEntry;
    use tempfile::TempDir;

    use super::*;

    #[ctor::ctor]
    fn init_logger_for_test() {
        use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

        TermLogger::init(
            LevelFilter::Debug,
            Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        );
    }

    #[test]
    fn should_split_metadata_rolls_single_leader() {
        let mut entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:02:56Z").unwrap(),
            )),
        ];

        for entry in entries.iter_mut() {
            entry.push_tag("camera:canonp".to_string());
        }

        let borrowed_entries: Vec<&MetadataEntryType> = entries.iter().by_ref().collect();
        let rolls = split_metadata_rolls(&borrowed_entries);

        assert_eq!(1, rolls.len());
        let roll = rolls.get(0).unwrap();
        assert_eq!(3, roll.entries().len());
    }

    #[test]
    fn should_split_metadata_rolls_multiple_leaders() {
        let mut entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:02:56Z").unwrap(),
            )),
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T05:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:02:56Z").unwrap(),
            )),
        ];

        for entry in entries.iter_mut() {
            entry.push_tag("camera:canonp".to_string());
        }

        let borrowed_entries: Vec<&MetadataEntryType> = entries.iter().by_ref().collect();
        let rolls = split_metadata_rolls(&borrowed_entries);

        assert_eq!(2, rolls.len());
        for roll in rolls {
            assert_eq!(3, roll.entries().len());
        }
    }

    #[test]
    fn should_group_metadata_rolls_based_on_camera() {
        let mut camera_1_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:02:56Z").unwrap(),
            )),
        ];
        let mut camera_2_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T05:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:02:56Z").unwrap(),
            )),
        ];

        for entry in camera_1_entries.iter_mut() {
            entry.push_tag("camera:canonp".to_string());
        }
        for entry in camera_2_entries.iter_mut() {
            entry.push_tag("camera:yashicamat".to_string());
        }

        let mut entries = Vec::new();
        entries.append(&mut camera_1_entries);
        entries.append(&mut camera_2_entries);

        assert_eq!(6, entries.len());

        let entry_groups = group_entries_by_camera(&entries);

        assert_eq!(2, entry_groups.len());
        assert!(entry_groups.contains_key(&Some(&"camera:canonp".to_string())));
        assert!(entry_groups.contains_key(&Some(&"camera:yashicamat".to_string())));

        for (_, entries) in entry_groups {
            assert_eq!(3, entries.len());
        }
    }

    #[test]
    fn should_write_importable_zip_file_successfully() -> Result<(), Error> {
        let temp_outputdir = TempDir::new()?;
        let mut camera_1_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:02:56Z").unwrap(),
            )),
        ];
        let mut camera_2_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T05:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:02:56Z").unwrap(),
            )),
        ];

        for entry in camera_1_entries.iter_mut() {
            entry.push_tag("camera:canonp".to_string());
        }
        for entry in camera_2_entries.iter_mut() {
            entry.push_tag("camera:yashicamat".to_string());
        }

        let mut entries = Vec::new();
        entries.append(&mut camera_1_entries);
        entries.append(&mut camera_2_entries);

        assert_eq!(6, entries.len());

        let (process_count, total_count) =
            split_metadata_entries_into_rolls(&entries, temp_outputdir.path());
        assert_eq!(2, process_count);
        assert_eq!(6, total_count);

        // Assume expected filenames
        let mut canonp_zip_filename = temp_outputdir.path().to_path_buf();
        let mut yashicamat_zip_filename = temp_outputdir.path().to_path_buf();

        canonp_zip_filename.push("hp5_plus_-_1600_canonp_0.zip");
        yashicamat_zip_filename.push("hp5_plus_-_1600_yashicamat_0.zip");

        assert!(canonp_zip_filename.exists());
        assert!(canonp_zip_filename.is_file());

        assert!(yashicamat_zip_filename.exists());
        assert!(yashicamat_zip_filename.is_file());

        // Attempt to load ZIP files
        let json_result = dayone_export_zip_to_json(canonp_zip_filename);
        assert!(matches!(json_result, Ok(_)));

        let json_result = dayone_export_zip_to_json(yashicamat_zip_filename);
        assert!(matches!(json_result, Ok(_)));

        Ok(())
    }

    #[test]
    fn should_write_importable_zip_file_successfully_ignoring_nocamera_roll() -> Result<(), Error> {
        let temp_outputdir = TempDir::new()?;
        let mut camera_1_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T04:02:56Z").unwrap(),
            )),
        ];
        let mut camera_2_entries = vec![
            MetadataEntryType::Leader(LeaderEntry::fake(
                "# HP5 Plus @ 1600 \\- 35mm \\(Canon P\\)".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T05:04:56Z").unwrap(),
                "HP5 Plus @ 1600".to_string(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "1".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:01:56Z").unwrap(),
            )),
            MetadataEntryType::Frame(FrameEntry::fake(
                "2".to_string(),
                "Text".to_string(),
                DateTime::parse_from_rfc3339("2025-01-02T06:02:56Z").unwrap(),
            )),
        ];

        for entry in camera_2_entries.iter_mut() {
            entry.push_tag("camera:yashicamat".to_string());
        }

        let mut entries = Vec::new();
        entries.append(&mut camera_1_entries);
        entries.append(&mut camera_2_entries);

        assert_eq!(6, entries.len());

        let (process_count, total_count) =
            split_metadata_entries_into_rolls(&entries, temp_outputdir.path());
        assert_eq!(1, process_count);
        assert_eq!(6, total_count);

        // Assume expected filenames
        let mut yashicamat_zip_filename = temp_outputdir.path().to_path_buf();
        yashicamat_zip_filename.push("hp5_plus_-_1600_yashicamat_0.zip");

        assert!(yashicamat_zip_filename.exists());
        assert!(yashicamat_zip_filename.is_file());

        let json_result = dayone_export_zip_to_json(yashicamat_zip_filename);
        assert!(matches!(json_result, Ok(_)));

        Ok(())
    }
}
