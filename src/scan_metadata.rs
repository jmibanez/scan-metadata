use clap::Parser;
use log::{error, warn, LevelFilter};
use regex::Regex;
use rexiv2::LogLevel;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use thiserror::Error;
use zip::ZipArchive;

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use crate::cli_message;
use crate::exif::{
    get_default_processor, get_experimental_processor, ExifProcessor, ExifProcessorOptions,
};
use crate::models::{to_metadata_entries, CameraLensProfile, DayOneExport, MetadataEntry};
use crate::util;

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

    /// EXPERIMENTAL: Use pure Rust EXIF implementation
    #[arg(long)]
    experimental_exif: bool,

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

#[derive(Error, Debug)]
pub enum MetadataReadError {
    #[error("Can't open file: {0}")]
    FileError(#[from] std::io::Error),

    #[error("Not a valid ZIP file: {0}")]
    ZipFileError(#[from] zip::result::ZipError),

    #[error("Malformed JSON data in export: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Malformed YAML data in camera profiles: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("Malformed version for JSON export, expected '1.0' got '{0}'")]
    InvalidVersionError(String),
}

fn dayone_export_zip_to_json(
    dayone_export_zip: PathBuf,
) -> Result<DayOneExport, MetadataReadError> {
    let f = File::open(dayone_export_zip)?;
    let mut zip = ZipArchive::new(f)?;
    let result = zip.by_name("Journal.json")?;
    let json: DayOneExport = serde_json::from_reader(result)?;

    if json.metadata.version != "1.0" {
        Err(MetadataReadError::InvalidVersionError(
            json.metadata.version.to_string(),
        ))
    } else {
        Ok(json)
    }
}

fn read_camera_profiles(
    camera_profiles_file: Option<PathBuf>,
) -> Result<Option<Vec<CameraLensProfile>>, MetadataReadError> {
    match camera_profiles_file {
        Some(file) => {
            let f = File::open(file)?;
            let yaml = serde_yaml::from_reader(f)?;
            Ok(Some(yaml))
        }
        None => Ok(None),
    }
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

pub fn scan_metadata() -> Result<(), ProgramError> {
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
    let camera_profiles = read_camera_profiles(args.profiles)?;

    let metadata_entries: Vec<MetadataEntry> = to_metadata_entries(json, camera_profiles);

    let proc: &dyn ExifProcessor = if args.experimental_exif {
        &get_experimental_processor()
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::DateTime;

    use super::*;
    use crate::exif::ExifTag;

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
