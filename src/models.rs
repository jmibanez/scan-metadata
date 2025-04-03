use chrono::{DateTime, Local, Timelike};
use regex::Regex;
use serde::Deserialize;

use std::path::Path;
use std::process::Command;

// #[derive(Deserialize)]
// struct DayOneExportMetadata{
//     version: String,
// }

#[derive(Clone, Deserialize, Debug)]
struct LongLat {
    longitude: f64,
    latitude: f64,
}

#[derive(Clone, Deserialize, Debug)]
struct DayOneRegion {
    center: LongLat,
    radius: f32,
}

#[derive(Clone, Deserialize, Debug)]
struct DayOneLocation {
    region: Option<DayOneRegion>,
    country: Option<String>,
    #[serde(rename = "administrativeArea")]
    administrative_area: Option<String>,
    #[serde(rename = "timeZoneName")]
    time_zone_name: Option<String>,
}

#[derive(Deserialize)]
struct DayOneExportEntry {
    location: Option<DayOneLocation>,
    tags: Vec<String>,
    #[serde(rename = "creationDate")]
    creation_date: String,
    text: String,
}

#[derive(Deserialize)]
pub struct DayOneExport {
    // metadata: DayOneExportMetadata,
    entries: Vec<DayOneExportEntry>,
}

#[derive(Debug)]
enum TagValue {
    String(String),
    List(Vec<String>),
}

#[derive(Debug)]
struct ExifTag {
    name: String,
    value: TagValue,
}

trait ExifTagTrait {
    fn to_exif_tag(&self, name: &str) -> ExifTag;
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

#[derive(Debug)]
pub struct MetadataEntry {
    frame_count: String,
    entry_date: String,
    location: Option<DayOneLocation>,
    entry_tags: Vec<String>,
    exif_tags: Vec<ExifTag>,
}

fn parse_frame_count(text: String) -> String {
    text.lines()
        .next()
        .and_then(|header| header.split_whitespace().nth(1))
        .unwrap_or("0")
        .parse()
        .unwrap()
}

pub fn to_metadata_entries(json: DayOneExport) -> Vec<MetadataEntry> {
    json.entries
        .iter()
        .map(|e| {
            let mut entry = MetadataEntry::new(
                e.text.clone(),
                e.creation_date.clone(),
                e.location.clone(),
                e.tags.clone(),
            );
            entry.populate_tags();
            entry
        })
        .collect()
}

impl MetadataEntry {
    fn new(
        raw_frame_count: String,
        entry_date: String,
        location: Option<DayOneLocation>,
        raw_entry_tags: Vec<String>,
    ) -> Self {
        let exif_tags = Vec::new();
        let mut entry_tags = raw_entry_tags.clone();
        let frame_count = parse_frame_count(raw_frame_count);
        entry_tags.sort();
        Self {
            frame_count,
            entry_date,
            location,
            entry_tags,
            exif_tags,
        }
    }

    pub fn frame_count(&self) -> &String {
        &self.frame_count
    }

    pub fn populate_tags(&mut self) {
        let munged_datetime_tag = self
            .munge_date_with_framecount()
            .to_exif_tag("DateTimeOriginal");
        self.exif_tags.push(munged_datetime_tag);

        self.populate_location_tags();
        self.populate_from_entry_tags();
    }

    fn populate_location_tags(&mut self) {
        if let Some(location) = &self.location {
            if let Some(region) = &location.region {
                let lat = region.center.latitude.to_string();
                let lon = region.center.longitude.to_string();
                let radius = region.radius.to_string();

                let gps_lat_tag = lat.to_exif_tag("GPSLatitude");
                let gps_lon_tag = lon.to_exif_tag("GPSLongitude");
                let gps_lat_ref_tag = lat.to_exif_tag("GPSLatitudeRef");
                let gps_lon_ref_tag = lon.to_exif_tag("GPSLongitudeRef");
                let gps_hpos_error_tag = radius.to_exif_tag("GPSHPositioningError");

                self.exif_tags.push(gps_lat_tag);
                self.exif_tags.push(gps_lon_tag);
                self.exif_tags.push(gps_lat_ref_tag);
                self.exif_tags.push(gps_lon_ref_tag);
                self.exif_tags.push(gps_hpos_error_tag);
            }

            if let Some(country) = &location.country {
                let country_tag = country.to_exif_tag("Country");
                self.exif_tags.push(country_tag);
            }

            if let Some(admin_area) = &location.administrative_area {
                let admin_area_tag = admin_area.to_exif_tag("State");
                self.exif_tags.push(admin_area_tag);
            }
        }
    }

    fn populate_from_entry_tags(&mut self) {
        let shutter_tag_matcher = Regex::new("(1/)?\\d+s").unwrap();
        let lens_focal_length_matcher = Regex::new(r"(\d+mm)").unwrap();

        self.entry_tags.retain(|tag| {
            if shutter_tag_matcher.is_match(tag) {
                self.exif_tags
                    .push(tag.replace('s', "").to_exif_tag("ShutterSpeedValue"));
                return false;
            }

            if let Some(aperture_tag) = tag.strip_prefix("f/") {
                self.exif_tags
                    .push(aperture_tag.to_exif_tag("ApertureValue"));
                return false;
            }

            if tag.starts_with("lens:") {
                if let Some(captures) = lens_focal_length_matcher.captures(tag) {
                    self.exif_tags
                        .push(captures.get(1).unwrap().as_str().to_exif_tag("FocalLength"));
                }
            }

            if tag == "unindexed" || tag == "scanned" {
                return false;
            }

            true
        });

        // Replace film type tags (120, 135) with prefixed tags
        for tag in self.entry_tags.iter_mut() {
            if tag == "120" {
                *tag = "film:120".to_string();
            }
            if tag == "35mm" {
                *tag = "film:135".to_string();
            }
        }

        let keyword_tag = self.entry_tags.to_exif_tag("Keywords");
        self.exif_tags.push(keyword_tag);
    }

    fn munge_date_with_framecount(&mut self) -> String {
        let entry_date_as_date = DateTime::parse_from_rfc3339(&self.entry_date).unwrap();
        let munged_datetime = entry_date_as_date
            .with_second(self.frame_count.parse::<u32>().unwrap())
            .unwrap();
        if let Some(location) = &self.location {
            if let Some(tz_name) = &location.time_zone_name {
                let tz = tz_name
                    .parse::<chrono::FixedOffset>()
                    .unwrap_or_else(|_| *Local::now().offset());
                return munged_datetime.with_timezone(&tz).to_rfc3339();
            }
        }

        munged_datetime
            .with_timezone(&Local::now().offset().clone())
            .to_rfc3339()
    }

    pub fn write_to_exif(&self, filepath: &Path, overwrite_original: bool) {
        let args = self.to_exiftool_cmd_line(filepath, overwrite_original);
        println!("Updating tags for {}", filepath.display());
        let mut proc = Command::new("exiftool").args(&args).spawn().unwrap();
        let _result = proc.wait().unwrap();
    }

    pub fn to_exiftool_cmd_line(&self, filepath: &Path, overwrite_original: bool) -> Vec<String> {
        let mut args = Vec::new();

        if overwrite_original {
            args.push("-overwrite_original_in_place".to_string());
        }

        for tag in self.exif_tags.iter() {
            match &tag.value {
                TagValue::String(v) => args.push(format!("-{}={}", tag.name, v)),
                TagValue::List(l) => {
                    for e in l.iter() {
                        args.push(format!("-{}={}", tag.name, e));
                    }
                }
            };
        }

        args.push(filepath.to_str().unwrap().to_string());
        args
    }
}
