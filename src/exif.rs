use lazy_static::lazy_static;
use log::{debug, warn, LevelFilter};
use num::rational::Ratio;
use rexiv2::Metadata;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use crate::cli_message;
use crate::util;

#[derive(Debug)]
pub struct ExifTag {
    name: String,
    value: TagValue,
}

#[derive(Debug)]
pub enum TagValue {
    Numeric(i32),
    Rational(Ratio<i32>),
    Float(f64),
    String(String),
    List(Vec<String>),
}

pub trait ExifTagTrait {
    fn to_exif_tag(&self, name: &str) -> ExifTag;
}

impl ExifTagTrait for i32 {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::Numeric(*self),
        }
    }
}

impl ExifTagTrait for i8 {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::Numeric(*self as i32),
        }
    }
}

impl ExifTagTrait for Ratio<i32> {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::Rational(*self),
        }
    }
}

impl ExifTagTrait for f32 {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::Float(*self as f64),
        }
    }
}

impl ExifTagTrait for f64 {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::Float(*self),
        }
    }
}

impl ExifTagTrait for str {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::String(self.to_string()),
        }
    }
}

impl ExifTagTrait for String {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::String(self.clone()),
        }
    }
}

impl ExifTagTrait for Vec<String> {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        ExifTag {
            name: name.to_string(),
            value: TagValue::List(self.clone()),
        }
    }
}

pub struct ExifProcessorOptions {
    pub inplace: bool,
    pub dryrun: bool,
}

pub trait ExifProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &[ExifTag],
        options: &ExifProcessorOptions,
    ) -> bool;
}

struct ExifToolProcessor {}

impl ExifToolProcessor {
    fn to_exiftool_cmd_line(
        &self,
        filepath: &Path,
        exif_tags: &[ExifTag],
        options: &ExifProcessorOptions,
    ) -> Vec<String> {
        let mut args = Vec::new();

        unsafe {
            if util::LOG_LEVEL != LevelFilter::Debug {
                args.push("-q".to_string());
            } else {
                args.push("-v4".to_string());
            }
        }

        if options.inplace {
            args.push("-overwrite_original_in_place".to_string());
        }

        for tag in exif_tags.iter() {
            match &tag.value {
                TagValue::String(v) => args.push(format!("-{}={}", tag.name, v)),
                TagValue::Numeric(v) => args.push(format!("-{}={}", tag.name, v)),
                TagValue::Rational(v) => args.push(format!("-{}={}", tag.name, v)),
                TagValue::Float(v) => args.push(format!("-{}={}", tag.name, v)),
                TagValue::List(l) => {
                    for e in l.iter() {
                        args.push(format!("-{}={}", tag.name, e));
                    }
                }
            };
        }

        args.push(filepath.to_str().unwrap().to_string());
        debug!("Arguments to exiftool: {:#?}", args);
        args
    }
}

impl ExifProcessor for ExifToolProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &[ExifTag],
        options: &ExifProcessorOptions,
    ) -> bool {
        let args = self.to_exiftool_cmd_line(filepath, exif_tags, options);
        if !options.dryrun {
            cli_message!("Updating tags for {}", filepath.display());
            let maybe_proc = Command::new("exiftool").args(&args).spawn();
            if maybe_proc.is_err() {
                panic!("ERROR: Cannot update scans; exiftool not found in PATH. exiftool must be installed first.");
            }
            let result = maybe_proc.unwrap().wait().unwrap();
            result.success()
        } else {
            let cmd = args.join(" \\\n\t\t");
            cli_message!("Would have updated {}", filepath.display());
            cli_message!("\texiftool {}", cmd);
            cli_message!();
            true
        }
    }
}

lazy_static! {
    static ref TAG_TRANSLATIONS: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("DateTimeOriginal", "Exif.Photo.DateTimeOriginal");
        m.insert("GPSLatitude", "Exif.GPSInfo.GPSLatitude");
        m.insert("GPSLongitude", "Exif.GPSInfo.GPSLongitude");
        m.insert("GPSHPositioningError", "Exif.GPSInfo.GPSHPositioningError");
        m.insert("Country", "Xmp.iptcExt.CountryName");
        m.insert("State", "Xmp.iptcExt.ProvinceState");
        m.insert("FNumber", "Exif.Photo.FNumber");
        m.insert("ExposureTime", "Exif.Photo.ExposureTime");
        m.insert("MaxAperture", "Exif.Image.MaxApertureValue");
        m.insert("FocalLength", "Exif.Photo.FocalLength");
        m.insert("LensMake", "Xmp.exifEX.LensMake");
        m.insert("LensModel", "Xmp.exifEX.LensModel");
        m.insert("CameraLabel", "Xmp.xmpDM.cameraLabel");
        m.insert("UserComment", "Exif.Photo.UserComment");
        m.insert("Keywords", "Iptc.Application2.Keywords");
        m
    };
    static ref SPECIAL_CASED_TAG: HashSet<&'static str> = {
        let mut s = HashSet::new();
        s.insert("MinFocalLength");
        s.insert("MaxFocalLength");
        s.insert("MaxApertureAtMinFocal");
        s.insert("MaxApertureAtMaxFocal");
        s.insert("GPSLatitude");
        s.insert("GPSLongitude");
        s
    };
}

fn translate_tag_to_exiv(tag_name: &str) -> &str {
    TAG_TRANSLATIONS.get(&tag_name).unwrap_or(&tag_name)
}

struct ExperimentalExifProcessor {}

impl ExperimentalExifProcessor {
    fn apply_exif_tag(&self, meta: &Metadata, tag: &ExifTag) {
        let tag_name = translate_tag_to_exiv(tag.name.as_str());
        let result = match &tag.value {
            TagValue::String(v) => meta.set_tag_string(tag_name, v),
            TagValue::Numeric(v) => meta.set_tag_numeric(tag_name, *v),
            TagValue::Rational(v) => meta.set_tag_rational(tag_name, v),
            TagValue::Float(v) => {
                let ratio = Ratio::approximate_float(*v).unwrap();
                meta.set_tag_rational(tag_name, &ratio)
            }
            TagValue::List(l) => {
                let l_str: Vec<&str> = l.iter().map(|e| e.as_str()).collect();
                meta.set_tag_multiple_strings(tag_name, l_str.as_slice())
            }
        };

        if result.is_err() {
            let err = result.err().unwrap();
            warn!("Error writing {}: {}", tag_name, err);
        }
    }

    fn handle_special_case_tags(&self, meta: &Metadata, exif_tags: &[ExifTag]) {
        // Special case tags here

        let mut maybe_latitude: Option<f64> = None;
        let mut maybe_longitude: Option<f64> = None;

        let mut maybe_min_focal_length: Option<&String> = None;
        let mut maybe_max_focal_length: Option<&String> = None;
        let mut maybe_max_aperture_at_min: Option<&String> = None;
        let mut maybe_max_aperture_at_max: Option<&String> = None;

        for tag in exif_tags.iter() {
            if tag.name == "MinFocalLength" {
                if let TagValue::String(s) = &tag.value {
                    maybe_min_focal_length = Some(s);
                } else {
                    panic!("Unexpected type for MinFocalLength");
                }
            }
            if tag.name == "MaxFocalLength" {
                if let TagValue::String(s) = &tag.value {
                    maybe_max_focal_length = Some(s);
                } else {
                    panic!("Unexpected type for MaxFocalLength");
                }
            }
            if tag.name == "MaxApertureAtMinFocal" {
                if let TagValue::String(s) = &tag.value {
                    maybe_max_aperture_at_min = Some(s);
                } else {
                    panic!("Unexpected type for MaxApertureAtMinFocal");
                }
            }
            if tag.name == "MaxApertureAtMaxFocal" {
                if let TagValue::String(s) = &tag.value {
                    maybe_max_aperture_at_max = Some(s);
                } else {
                    panic!("Unexpected type for MaxApertureAtMaxFocal");
                }
            }

            if tag.name == "GPSLatitude" {
                if let TagValue::Float(s) = &tag.value {
                    maybe_latitude = Some(*s);
                } else {
                    panic!("Unexpected type for GPSLatitude");
                }
            }
            if tag.name == "GPSLongitude" {
                if let TagValue::Float(s) = &tag.value {
                    maybe_longitude = Some(*s);
                } else {
                    panic!("Unexpected type for GPSLongitude");
                }
            }
        }

        if let (
            Some(min_focal_length),
            Some(max_focal_length),
            Some(max_aperture_at_min),
            Some(max_aperture_at_max),
        ) = (
            maybe_min_focal_length,
            maybe_max_focal_length,
            maybe_max_aperture_at_min,
            maybe_max_aperture_at_max,
        ) {
            let mut lens_spec_vec = Vec::new();
            lens_spec_vec.push(min_focal_length.clone());
            lens_spec_vec.push(max_focal_length.clone());
            lens_spec_vec.push(max_aperture_at_min.clone());
            lens_spec_vec.push(max_aperture_at_max.clone());
            let lens_spec_tag = lens_spec_vec.to_exif_tag("Exif.Photo.LensSpecification");
            self.apply_exif_tag(meta, &lens_spec_tag);
        }

        if let (Some(latitude), Some(longitude)) = (maybe_latitude, maybe_longitude) {
            match meta.set_gps_info(&rexiv2::GpsInfo {
                latitude,
                longitude,
                altitude: 0.0,
            }) {
                Ok(_) => (),
                Err(e) => {
                    warn!(
                        "Could not set GPS info with values {} {}: {}",
                        latitude, longitude, e
                    );
                }
            };
        }
    }
}

impl ExifProcessor for ExperimentalExifProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &[ExifTag],
        options: &ExifProcessorOptions,
    ) -> bool {
        if !options.dryrun {
            cli_message!("EXPERIMENTAL Updating tags for {}", filepath.display());
            let meta = Metadata::new_from_path(filepath).unwrap();

            self.handle_special_case_tags(&meta, exif_tags);

            for tag in exif_tags.iter() {
                if SPECIAL_CASED_TAG.contains(tag.name.as_str()) {
                    continue;
                }
                self.apply_exif_tag(&meta, tag);
            }

            if !options.inplace {
                use std::fs::copy;
                let mut new_name = filepath.file_name().unwrap().to_os_string();
                new_name.push("_original");
                let newpath = filepath.with_file_name(new_name);
                match copy(filepath, newpath) {
                    Ok(_) => (),
                    Err(e) => {
                        warn!(
                            "Could not preserve original {}, falling back to in-place: {}",
                            filepath.display(),
                            e
                        );
                    }
                }
            }

            match meta.save_to_file(filepath) {
                Ok(()) => true,
                Err(e) => {
                    warn!(
                        "Could not update metadata for scan {}: {}",
                        filepath.display(),
                        e
                    );
                    false
                }
            }
        } else {
            cli_message!("EXPERIMENTAL Would have updated {}", filepath.display());
            true
        }
    }
}

pub fn get_default_processor() -> impl ExifProcessor {
    ExifToolProcessor {}
}

pub fn get_experimental_processor() -> impl ExifProcessor {
    ExperimentalExifProcessor {}
}
