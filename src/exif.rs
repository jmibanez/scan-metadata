use lazy_static::lazy_static;
use log::{debug, warn, LevelFilter};
use num::rational::Ratio;
use rexiv2::{set_log_level, LogLevel, Metadata};

use std::collections::HashMap;
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
        let ratio = Ratio::approximate_float(*self).unwrap();
        ExifTag {
            name: name.to_string(),
            value: TagValue::Rational(ratio),
        }
    }
}

impl ExifTagTrait for f64 {
    fn to_exif_tag(&self, name: &str) -> ExifTag {
        let ratio = Ratio::approximate_float(*self).unwrap();
        ExifTag {
            name: name.to_string(),
            value: TagValue::Rational(ratio),
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
            set_log_level(LogLevel::MUTE);

            let mut lens_spec: [String; 4] = Default::default();
            for tag in exif_tags.iter() {
                if tag.name == "MinFocalLength" {
                    if let TagValue::String(s) = &tag.value {
                        lens_spec[0] = s.clone()
                    }
                }
                if tag.name == "MaxFocalLength" {
                    if let TagValue::String(s) = &tag.value {
                        lens_spec[1] = s.clone()
                    }
                }
                if tag.name == "MaxApertureAtMinFocal" {
                    if let TagValue::String(s) = &tag.value {
                        lens_spec[2] = s.clone()
                    }
                }
                if tag.name == "MaxApertureAtMaxFocal" {
                    if let TagValue::String(s) = &tag.value {
                        lens_spec[3] = s.clone()
                    }
                }
            }

            let lens_spec_tag = Vec::from(lens_spec).to_exif_tag("Exif.Photo.LensSpecification");
            self.apply_exif_tag(&meta, &lens_spec_tag);

            for tag in exif_tags.iter() {
                if tag.name == "MinFocalLength"
                    || tag.name == "MaxFocalLength"
                    || tag.name == "MaxApertureAtMinFocal"
                    || tag.name == "MaxApertureAtMaxFocal"
                {
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
