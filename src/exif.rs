use lazy_static::lazy_static;
use log::{debug, warn, LevelFilter};
use num::rational::Ratio;
use rexiv2::{GpsInfo, Metadata};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use crate::cli_message;
use crate::util;

#[derive(Debug, PartialEq)]
pub struct ExifTag {
    pub name: String,
    pub value: TagValue,
}

#[derive(Debug, PartialEq)]
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

        if !util::is_log_level(LevelFilter::Debug) {
            args.push("-q".to_string());
        } else {
            args.push("-v4".to_string());
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
        m.insert("Country", "Xmp.photoshop.Country");
        m.insert("State", "Xmp.photoshop.State");
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

#[cfg_attr(test, mockall::automock)]
trait MetadataOperations {
    fn set_tag_string(&self, tag: &str, value: &str) -> rexiv2::Result<()>;
    fn set_tag_multiple_strings<'a>(&self, tag: &str, values: &'a [&'a str]) -> rexiv2::Result<()>;
    fn set_tag_numeric(&self, tag: &str, value: i32) -> rexiv2::Result<()>;
    fn set_tag_rational(&self, tag: &str, value: &Ratio<i32>) -> rexiv2::Result<()>;
    fn set_gps_info(&self, gps: &GpsInfo) -> rexiv2::Result<()>;
}

impl MetadataOperations for Metadata {
    fn set_tag_string(&self, tag: &str, value: &str) -> rexiv2::Result<()> {
        self.set_tag_string(tag, value)
    }

    fn set_tag_multiple_strings(&self, tag: &str, values: &[&str]) -> rexiv2::Result<()> {
        self.set_tag_multiple_strings(tag, values)
    }

    fn set_tag_numeric(&self, tag: &str, value: i32) -> rexiv2::Result<()> {
        self.set_tag_numeric(tag, value)
    }

    fn set_tag_rational(&self, tag: &str, value: &Ratio<i32>) -> rexiv2::Result<()> {
        self.set_tag_rational(tag, value)
    }

    fn set_gps_info(&self, gps: &GpsInfo) -> rexiv2::Result<()> {
        self.set_gps_info(gps)
    }
}

struct Rexiv2ExifProcessor {}

impl Rexiv2ExifProcessor {
    fn apply_exif_tag(&self, meta: &dyn MetadataOperations, tag: &ExifTag) {
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

    fn handle_special_case_tags(&self, meta: &dyn MetadataOperations, exif_tags: &[ExifTag]) {
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
            let lens_spec_vec = vec![
                min_focal_length.clone(),
                max_focal_length.clone(),
                max_aperture_at_min.clone(),
                max_aperture_at_max.clone(),
            ];
            let lens_spec_tag = lens_spec_vec.to_exif_tag("Exif.Photo.LensSpecification");
            self.apply_exif_tag(meta, &lens_spec_tag);
        }

        if let (Some(latitude), Some(longitude)) = (maybe_latitude, maybe_longitude) {
            match meta.set_gps_info(&GpsInfo {
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

impl ExifProcessor for Rexiv2ExifProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &[ExifTag],
        options: &ExifProcessorOptions,
    ) -> bool {
        if !options.dryrun {
            cli_message!("Updating tags for {}", filepath.display());
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
            cli_message!("Would have updated {}", filepath.display());
            true
        }
    }
}

pub fn get_default_processor() -> impl ExifProcessor {
    Rexiv2ExifProcessor {}
}

pub fn get_legacy_processor() -> impl ExifProcessor {
    ExifToolProcessor {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;

    fn fake_exif_tags() -> Vec<ExifTag> {
        let mut exif_tags = Vec::new();
        exif_tags.push("bar".to_exif_tag("Foo"));
        exif_tags.push("hey".to_exif_tag("Hey"));
        exif_tags.push(42.to_exif_tag("SomeInt"));

        exif_tags
    }

    #[test]
    fn exiftool_processor_should_yield_correct_cmd_line() {
        let proc = ExifToolProcessor {};
        let exif_tags = fake_exif_tags();

        assert_eq!(
            Vec::from([
                "-v4",
                "-overwrite_original_in_place",
                "-Foo=bar",
                "-Hey=hey",
                "-SomeInt=42",
                "/test/path"
            ]),
            proc.to_exiftool_cmd_line(
                Path::new("/test/path"),
                &exif_tags,
                &ExifProcessorOptions {
                    inplace: true,
                    dryrun: true,
                },
            )
        );

        assert_eq!(
            Vec::from([
                "-v4",
                "-overwrite_original_in_place",
                "-Foo=bar",
                "-Hey=hey",
                "-SomeInt=42",
                "/test/path"
            ]),
            proc.to_exiftool_cmd_line(
                Path::new("/test/path"),
                &exif_tags,
                &ExifProcessorOptions {
                    inplace: true,
                    dryrun: false,
                },
            )
        );

        assert_eq!(
            Vec::from(["-v4", "-Foo=bar", "-Hey=hey", "-SomeInt=42", "/test/path"]),
            proc.to_exiftool_cmd_line(
                Path::new("/test/path"),
                &exif_tags,
                &ExifProcessorOptions {
                    inplace: false,
                    dryrun: false,
                },
            )
        );
    }

    #[test]
    fn exiftool_processor_should_handle_various_value_types() {
        let proc = ExifToolProcessor {};
        let mut exif_tags = Vec::new();
        exif_tags.push("bar".to_exif_tag("Foo"));
        exif_tags.push("hey".to_string().to_exif_tag("Hey"));
        exif_tags.push(42.to_exif_tag("SomeInt"));
        exif_tags.push((42.42).to_exif_tag("SomeFloat"));
        let ratio = Ratio::new(3, 7);
        exif_tags.push(ratio.to_exif_tag("SomeRatio"));

        assert_eq!(
            Vec::from([
                "-v4",
                "-Foo=bar",
                "-Hey=hey",
                "-SomeInt=42",
                "-SomeFloat=42.42",
                "-SomeRatio=3/7",
                "/test/path"
            ]),
            proc.to_exiftool_cmd_line(
                Path::new("/test/path"),
                &exif_tags,
                &ExifProcessorOptions {
                    inplace: false,
                    dryrun: true,
                },
            )
        );
    }

    #[test]
    fn rexiv2_processor_apply_exif_tag_base_case() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta
            .expect_set_tag_string()
            .once()
            .returning(|_, _| Ok(()))
            .with(eq("TestTag"), eq("TestTagValue"));
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();

        let tag = "TestTagValue".to_exif_tag("TestTag");

        proc.apply_exif_tag(&mock_meta, &tag);
    }

    #[test]
    fn rexiv2_processor_apply_exif_tag_on_translated_string_tag() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta
            .expect_set_tag_string()
            .once()
            .returning(|_, _| Ok(()))
            .with(eq("Exif.Photo.DateTimeOriginal"), eq("TestTagValue"));
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();

        let tag = "TestTagValue".to_exif_tag("DateTimeOriginal");

        proc.apply_exif_tag(&mock_meta, &tag);
    }

    #[test]
    fn rexiv2_processor_ignore_regular_tags_in_handle_special_case_tags() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        let foo_tag = (-1.234).to_exif_tag("Foo");
        let bar_tag = (-5.6789).to_exif_tag("Bar");

        mock_meta.expect_set_tag_string().never();
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();
        mock_meta.expect_set_gps_info().never();

        proc.handle_special_case_tags(&mock_meta, &[foo_tag, bar_tag]);
    }

    #[test]
    fn rexiv2_processor_special_casing_ignore_gps_tags_if_either_missing() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta.expect_set_gps_info().never();
        mock_meta.expect_set_tag_string().never();
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();

        let lon = (-1.234).to_exif_tag("GPSLongitude");
        let lat = (-5.6789).to_exif_tag("GPSLatitude");

        proc.handle_special_case_tags(&mock_meta, &[lon]);

        mock_meta.checkpoint();

        proc.handle_special_case_tags(&mock_meta, &[lat]);
    }

    #[test]
    fn rexiv2_processor_handle_special_cased_gps_tags() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta
            .expect_set_gps_info()
            .once()
            .returning(|_| Ok(()))
            .with(eq(GpsInfo {
                longitude: -1.234,
                latitude: -5.6789,
                altitude: 0.0,
            }));
        mock_meta.expect_set_tag_string().never();
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();

        let lon = (-1.234).to_exif_tag("GPSLongitude");
        let lat = (-5.6789).to_exif_tag("GPSLatitude");

        proc.handle_special_case_tags(&mock_meta, &[lon, lat]);
    }

    #[test]
    fn rexiv2_processor_special_casing_ignore_lens_info_tag_when_missing_some() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta.expect_set_gps_info().never();
        mock_meta.expect_set_tag_string().never();
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();
        mock_meta.expect_set_tag_multiple_strings().never();

        let min_len = "a".to_exif_tag("MinFocalLength");
        let max_aperture_min = "c".to_exif_tag("MaxApertureAtMinFocal");
        let max_aperture_max = "d".to_exif_tag("MaxApertureAtMaxFocal");

        proc.handle_special_case_tags(&mock_meta, &[min_len, max_aperture_min, max_aperture_max]);
    }

    #[test]
    fn rexiv2_processor_handle_special_cased_lens_info_tags() {
        let proc = Rexiv2ExifProcessor {};
        let mut mock_meta = MockMetadataOperations::new();

        mock_meta
            .expect_set_tag_multiple_strings()
            .once()
            .returning(|_, _| Ok(()))
            .withf(|tag, val| {
                tag == "Exif.Photo.LensSpecification" && val == vec!["a", "b", "c", "d"]
            });
        mock_meta.expect_set_gps_info().never();
        mock_meta.expect_set_tag_string().never();
        mock_meta.expect_set_tag_numeric().never();
        mock_meta.expect_set_tag_rational().never();

        let min_len = "a".to_exif_tag("MinFocalLength");
        let max_len = "b".to_exif_tag("MaxFocalLength");
        let max_aperture_min = "c".to_exif_tag("MaxApertureAtMinFocal");
        let max_aperture_max = "d".to_exif_tag("MaxApertureAtMaxFocal");

        proc.handle_special_case_tags(
            &mock_meta,
            &[min_len, max_len, max_aperture_min, max_aperture_max],
        );
    }
}
