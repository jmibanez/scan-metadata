use clap::Parser;
use directories::ProjectDirs;
use log::{LevelFilter, error, warn};
use regex::Regex;
use rexiv2::LogLevel;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use thiserror::Error;

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scan_metadata::cli_message;
use scan_metadata::exif::{
    ExifProcessor, ExifProcessorOptions, get_default_processor, get_legacy_processor,
};
use scan_metadata::models::{
    CameraLensProfile, MetadataEntry, MetadataReadError, dayone_export_zip_to_json,
    to_metadata_entries,
};
use scan_metadata::util;

const PROJECT_QUALIFER: &str = "com";
const PROJECT_ORGANIZATION: &str = "jmibanez";
const PROJECT_APPNAME: &str = "scan-metadata";

#[derive(Parser)]
struct Args {
    /// Quiet; minimize output to errors
    #[arg(short, long)]
    quiet: bool,

    /// Turn on debug logging
    #[arg(long)]
    debug: bool,

    /// Modify scans in place
    #[arg(short, long)]
    inplace: bool,

    /// Dry run; show what would be done to the scans
    #[arg(long)]
    dryrun: bool,

    /// Legacy: Fork exiftool instead of using internal EXIF processor
    #[arg(long)]
    legacy_exif: bool,

    /// Use YAML with camera and lens profiles
    #[arg(short, long)]
    profiles: Option<PathBuf>,

    /// The path to the exported metadata, as a ZIP file
    dayone_export_zip: PathBuf,

    #[arg(num_args=1.., required=true)]
    /// Scan files to update
    filelist: Vec<PathBuf>,
}

#[derive(Error, Debug)]
pub enum ProgramError {
    #[error("Invalid metadata")]
    MetadataError(#[from] MetadataReadError),
}

fn read_camera_profiles_fallback_to_prefs(
    camera_profiles_file: Option<PathBuf>,
) -> Result<Option<Vec<CameraLensProfile>>, MetadataReadError> {
    match camera_profiles_file {
        Some(file) => read_camera_profiles_yaml(file.as_path()),
        None => {
            if let Some(project) =
                ProjectDirs::from(PROJECT_QUALIFER, PROJECT_ORGANIZATION, PROJECT_APPNAME)
            {
                let mut user_prefs_profiles = project.config_dir().to_path_buf();
                user_prefs_profiles.push("camera_profiles.yaml");
                if user_prefs_profiles.exists() {
                    read_camera_profiles_yaml(user_prefs_profiles.as_path())
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
    }
}

fn read_camera_profiles_yaml(
    camera_profiles_file: &Path,
) -> Result<Option<Vec<CameraLensProfile>>, MetadataReadError> {
    let f = File::open(camera_profiles_file)?;
    let yaml = serde_yaml::from_reader(f)?;
    Ok(Some(yaml))
}

fn match_files_to_entries(
    proc: &dyn ExifProcessor,
    filelist: Vec<PathBuf>,
    metadata_entries: Vec<MetadataEntry>,
    overwrite: bool,
    dryrun: bool,
) -> (usize, usize, usize) {
    let mut sorted_filelist = filelist.clone();
    sorted_filelist.sort();
    let metadata_map: HashMap<_, _> = metadata_entries
        .iter()
        .map(|e| (e.frame_count(), e))
        .collect();

    let entry_filename_matcher = Regex::new("(([1-9])|([1-9][0-9]))$").unwrap();
    let mut process_count = 0;
    let opt = ExifProcessorOptions {
        dryrun,
        inplace: overwrite,
    };
    for scan in sorted_filelist.iter() {
        let filename_stem = scan.file_stem().unwrap().to_str().unwrap();
        if let Some(scan_frame_count_capture) = entry_filename_matcher.captures(filename_stem) {
            let scan_frame_count = scan_frame_count_capture
                .get(1)
                .unwrap()
                .as_str()
                .to_string();
            if let Some(entry) = metadata_map.get(&scan_frame_count) {
                if proc.write_out_exif(scan, entry.exif_tags(), &opt) {
                    process_count += 1;
                }
            } else {
                warn!("Did not find metadata entry for frame {}", scan_frame_count);
            }
        }
    }

    (process_count, filelist.len(), metadata_entries.len())
}

fn update_scan_exif() -> Result<(), ProgramError> {
    let args = Args::parse();

    if args.quiet {
        rexiv2::set_log_level(LogLevel::MUTE);
        util::set_log_level(LevelFilter::Off);
    } else if args.debug {
        rexiv2::set_log_level(LogLevel::WARN);
        util::set_log_level(LevelFilter::Debug);
    }
    let level = util::get_log_level();

    let _ = TermLogger::init(
        level,
        Config::default(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    );

    let json = dayone_export_zip_to_json(args.dayone_export_zip)?;
    let camera_profiles = read_camera_profiles_fallback_to_prefs(args.profiles)?;

    let metadata_entries: Vec<MetadataEntry> = to_metadata_entries(json, camera_profiles);

    let proc: &dyn ExifProcessor = if args.legacy_exif {
        &get_legacy_processor()
    } else {
        &get_default_processor()
    };

    let (process_count, file_count, metadata_count) = match_files_to_entries(
        proc,
        args.filelist,
        metadata_entries,
        args.inplace,
        args.dryrun,
    );

    cli_message!(
        "Processed {}/{} scan(s); found {} metadata entries.",
        process_count,
        file_count,
        metadata_count
    );

    Ok(())
}

fn main() -> ExitCode {
    let result = update_scan_exif();
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(ProgramError::MetadataError(e)) => {
            error!("Could not read metadata for scans: {}", e);
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::DateTime;

    use super::*;
    use scan_metadata::exif::ExifTag;

    struct TestExifProcessor {}
    impl ExifProcessor for TestExifProcessor {
        fn write_out_exif(&self, _: &Path, _: &[ExifTag], _: &ExifProcessorOptions) -> bool {
            true
        }
    }

    fn test_proc() -> impl ExifProcessor {
        TestExifProcessor {}
    }

    #[test]
    fn should_match_base_case_filenames() {
        let metadata = MetadataEntry::fake(
            "1".to_string(),
            "# 1 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
        );
        let filelist = vec![Path::new("/tmp/test/hp5cp160_001.tif").to_path_buf()];
        let metadata_entries = vec![metadata];
        let proc = &test_proc();

        let (process_count, file_count, metadata_count) =
            match_files_to_entries(proc, filelist, metadata_entries, false, true);
        assert_eq!(1, process_count, "Should have matched against 1 file");
        assert_eq!(1, file_count, "Should have seen 1 file to process");
        assert_eq!(1, metadata_count, "Should have 1 metadata entry");
    }

    #[test]
    fn should_match_two_digit_framecount_filenames() {
        let metadata = MetadataEntry::fake(
            "12".to_string(),
            "# 12 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
        );
        let filelist = vec![Path::new("/tmp/test/hp5cp160_012.tif").to_path_buf()];
        let metadata_entries = vec![metadata];
        let proc = &test_proc();

        let (process_count, file_count, metadata_count) =
            match_files_to_entries(proc, filelist, metadata_entries, false, true);
        assert_eq!(1, process_count, "Should have matched against 1 file");
        assert_eq!(1, file_count, "Should have seen 1 file to process");
        assert_eq!(1, metadata_count, "Should have 1 metadata entry");
    }

    #[test]
    fn should_match_filenames_with_noise_digits_in_middle() {
        let metadata = MetadataEntry::fake(
            "12".to_string(),
            "# 12 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
        );
        let filelist = vec![Path::new("/tmp/test/hp5cp160_001_012.tif").to_path_buf()];
        let metadata_entries = vec![metadata];
        let proc = &test_proc();

        let (process_count, file_count, metadata_count) =
            match_files_to_entries(proc, filelist, metadata_entries, false, true);
        assert_eq!(1, process_count, "Should have matched against 1 file");
        assert_eq!(1, file_count, "Should have seen 1 file to process");
        assert_eq!(1, metadata_count, "Should have 1 metadata entry");
    }

    #[test]
    fn should_match_filenames_with_only_digits() {
        let metadata = MetadataEntry::fake(
            "12".to_string(),
            "# 12 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
        );
        let filelist = vec![Path::new("/tmp/test/000001012.tif").to_path_buf()];
        let metadata_entries = vec![metadata];
        let proc = &test_proc();

        let (process_count, file_count, metadata_count) =
            match_files_to_entries(proc, filelist, metadata_entries, false, true);
        assert_eq!(1, process_count, "Should have matched against 1 file");
        assert_eq!(1, file_count, "Should have seen 1 file to process");
        assert_eq!(1, metadata_count, "Should have 1 metadata entry");
    }

    #[test]
    fn should_match_filenames_digits_only_consider_last_two_digits() {
        let metadata = MetadataEntry::fake(
            "12".to_string(),
            "# 12 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
        );
        let filelist = vec![Path::new("/tmp/test/000001912.tif").to_path_buf()];
        let metadata_entries = vec![metadata];
        let proc = &test_proc();

        let (process_count, file_count, metadata_count) =
            match_files_to_entries(proc, filelist, metadata_entries, false, true);
        assert_eq!(1, process_count, "Should have matched against 1 file");
        assert_eq!(1, file_count, "Should have seen 1 file to process");
        assert_eq!(1, metadata_count, "Should have 1 metadata entry");
    }
}
