use log::error;

use std::process::ExitCode;

use crate::scan_metadata::{scan_metadata, ProgramError};

pub mod exif;
pub mod models;
pub mod scan_metadata;
pub mod util;

fn main() -> ExitCode {
    let result = scan_metadata();
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(ProgramError::MetadataError(e)) => {
            error!("Could not read metadata for scans: {}", e);
            ExitCode::from(2)
        }
    }
}
