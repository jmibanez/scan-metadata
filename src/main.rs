use clap::Parser;
use regex::Regex;
use thiserror::Error;
use zip::ZipArchive;

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::exif::{
    get_default_processor, get_experimental_processor, ExifProcessor, ExifProcessorOptions,
};
use crate::models::{to_metadata_entries, CameraLensProfile, DayOneExport, MetadataEntry};

pub mod exif;
pub mod models;

#[derive(Parser)]
struct Args {
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

    #[error("Malformed JSON, expected '1.0' got {0}")]
    InvalidVersionError(String),
}

fn dayone_export_zip_to_json(
    dayone_export_zip: PathBuf,
) -> Result<DayOneExport, MetadataReadError> {
    let f = File::open(dayone_export_zip)?;
    let mut zip = ZipArchive::new(f)?;
    let result = zip.by_name("Journal.json")?;
    let json = serde_json::from_reader(result)?;

    Ok(json)
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
    proc: &Box<dyn ExifProcessor>,
    filelist: Vec<PathBuf>,
    metadata_entries: Vec<MetadataEntry>,
    overwrite: bool,
    dryrun: bool,
) {
    let mut sorted_filelist = filelist.clone();
    sorted_filelist.sort();
    let metadata_map: HashMap<_, _> = metadata_entries
        .iter()
        .map(|e| (e.frame_count(), e))
        .collect();

    let entry_filename_matcher = Regex::new("(.*)(0+)(\\d+)").unwrap();
    let mut process_count = 0;
    let opt = ExifProcessorOptions {
        dryrun,
        inplace: overwrite,
    };
    for scan in sorted_filelist.iter() {
        let filename_stem = scan.file_stem().unwrap().to_str().unwrap();
        if let Some(scan_frame_count_capture) = entry_filename_matcher.captures(filename_stem) {
            let scan_frame_count = scan_frame_count_capture
                .get(3)
                .unwrap()
                .as_str()
                .to_string();
            if let Some(entry) = metadata_map.get(&scan_frame_count) {
                if proc.write_out_exif(scan, entry.exif_tags(), &opt) {
                    process_count += 1;
                }
            }
        }
    }

    println!(
        "Processed {}/{} scan(s); found {} metadata entries.",
        process_count,
        filelist.len(),
        metadata_entries.len()
    );
}

fn scan_metadata() -> Result<(), ProgramError> {
    let args = Args::parse();

    let json = dayone_export_zip_to_json(args.dayone_export_zip)?;
    let camera_profiles = read_camera_profiles(args.profiles)?;

    let metadata_entries: Vec<MetadataEntry> = to_metadata_entries(json, camera_profiles);
    let proc;

    if args.experimental_exif {
        proc = get_experimental_processor();
    } else {
        proc = get_default_processor();
    }

    match_files_to_entries(
        &proc,
        args.filelist,
        metadata_entries,
        args.inplace,
        args.dryrun,
    );

    Ok(())
}

fn main() -> ExitCode {
    let result = scan_metadata();
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(ProgramError::MetadataError(e)) => {
            eprintln!("ERROR: Could not read metadata for scans: {}", e);
            ExitCode::from(2)
        }
    }
}
