use clap::Parser;
use regex::Regex;
use thiserror::Error;
use zip::ZipArchive;

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use crate::models::{DayOneExport, MetadataEntry, to_metadata_entries};

pub mod models;

#[derive(Parser)]
struct Args {
    /// Modify scans in place
    #[arg(short, long)]
    inplace: bool,

    /// Dry run; show what would be done to the scans
    #[arg(long)]
    dryrun: bool,

    /// The path to the exported metadata, as a ZIP file
    dayone_export_zip: PathBuf,

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
    #[error("Can't open file")]
    FileError(#[from] std::io::Error),

    #[error("Not a valid ZIP file")]
    ZipFileError(#[from] zip::result::ZipError),

    #[error("Malformed JSON data in export")]
    JsonError(#[from] serde_json::Error),

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

fn match_files_to_entries(
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
    for scan in sorted_filelist.iter() {
        let filename_stem = scan.file_stem().unwrap().to_str().unwrap();
        if let Some(scan_frame_count_capture) = entry_filename_matcher.captures(filename_stem) {
            let scan_frame_count = scan_frame_count_capture
                .get(3)
                .unwrap()
                .as_str()
                .to_string();
            if let Some(entry) = metadata_map.get(&scan_frame_count) {
                if !dryrun {
                    entry.write_to_exif(scan, overwrite);
                } else {
                    let args = entry.to_exiftool_cmd_line(scan, overwrite);
                    let cmd = args.join(" ");
                    println!("Would have updated {}", scan.display());
                    println!("\t{}", cmd);
                }
            }
        }
    }
}

fn main() -> Result<(), ProgramError> {
    let args = Args::parse();

    let json = dayone_export_zip_to_json(args.dayone_export_zip)?;
    let metadata_entries: Vec<MetadataEntry> = to_metadata_entries(json);

    let filelist = args
        .filelist
        .iter()
        .map(|f| f.to_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    println!("Filelist: {}", filelist);
    match_files_to_entries(args.filelist, metadata_entries, args.inplace, args.dryrun);

    Ok(())
}
