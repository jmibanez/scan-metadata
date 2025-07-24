use chrono::{DateTime, FixedOffset, Local, Timelike};
use chrono_tz::Tz;
use log::{debug, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use std::{
    fs::{File, remove_file},
    path::{Path, PathBuf},
};

use crate::{
    camera_profiles::{CameraLensProfile, CameraProfileMap},
    exif::{ExifTag, ExifTagTrait},
};

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

#[derive(Error, Debug)]
pub enum MetadataWriteError {
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

#[derive(Deserialize, Serialize)]
pub struct DayOneExportMetadata {
    pub version: String,
}

#[derive(Clone, Default, Deserialize, Debug, Serialize)]
struct LongLat {
    longitude: f64,
    latitude: f64,
}

#[derive(Clone, Default, Deserialize, Debug, Serialize)]
struct DayOneRegion {
    center: LongLat,
    radius: f32,
}

#[derive(Clone, Default, Deserialize, Debug, Serialize)]
struct DayOneWeather {
    #[serde(rename = "sunriseDate", with = "json_date")]
    sunrise_date: DateTime<FixedOffset>,
    #[serde(rename = "sunsetDate", with = "json_date")]
    sunset_date: DateTime<FixedOffset>,

    #[serde(flatten)]
    _other: serde_json::Value,
}

#[derive(Clone, Default, Deserialize, Debug, Serialize)]
struct DayOneLocation {
    region: Option<DayOneRegion>,
    country: Option<String>,
    #[serde(rename = "administrativeArea")]
    administrative_area: Option<String>,
    #[serde(rename = "timeZoneName")]
    time_zone_name: Option<String>,

    #[serde(flatten)]
    _other: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Default, Serialize)]
struct DayOneExportEntry {
    location: Option<DayOneLocation>,
    tags: Vec<String>,
    #[serde(rename = "creationDate", with = "json_date")]
    creation_date: DateTime<FixedOffset>,
    text: String,
    weather: Option<DayOneWeather>,

    #[serde(flatten)]
    _other: serde_json::Value,
}

#[derive(Deserialize, Serialize)]
pub struct DayOneExport {
    pub metadata: DayOneExportMetadata,
    entries: Vec<DayOneExportEntry>,
}

pub fn dayone_export_zip_to_json(
    dayone_export_zip: PathBuf,
) -> Result<DayOneExport, MetadataReadError> {
    let f = File::open(dayone_export_zip)?;
    let mut zip = ZipArchive::new(f)?;
    let result = zip.by_name("Journal.json")?;
    let json: DayOneExport = serde_json::from_reader(result)?;

    if json.metadata.version == "1.0" {
        Ok(json)
    } else {
        Err(MetadataReadError::InvalidVersionError(
            json.metadata.version.to_string(),
        ))
    }
}

#[derive(Debug)]
pub enum MetadataEntryType {
    Leader(LeaderEntry),
    Frame(FrameEntry),
}

#[derive(Debug, Default)]
pub struct MetadataEntry {
    entry: DayOneExportEntry,
    text: String,
    entry_date: DateTime<FixedOffset>,
    location: Option<DayOneLocation>,
    tags: Vec<String>,
}

#[derive(Debug, Default)]
pub struct LeaderEntry {
    entry: MetadataEntry,
    emulsion_name: String,
    entry_date: DateTime<FixedOffset>,
}

#[derive(Debug, Default)]
pub struct FrameEntry {
    entry: MetadataEntry,
    frame_count: String,
    exif_tags: Vec<ExifTag>,
}

#[derive(Debug)]
pub struct Roll<'a> {
    // id: String,
    camera: String,
    emulsion: String,
    entries: Vec<&'a MetadataEntryType>,
    start_date: DateTime<FixedOffset>,
}

fn parse_frame_count(text: &str) -> String {
    let candidate: String = text
        .lines()
        .next()
        .and_then(|header| header.split_whitespace().nth(1))
        .unwrap_or("0")
        .parse()
        .unwrap();

    let candidate_maybe_number = candidate.parse::<u32>();
    if candidate_maybe_number.is_ok() {
        candidate
    } else {
        String::default()
    }
}

mod json_date {
    use chrono::{DateTime, FixedOffset};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&s).map_err(serde::de::Error::custom)
    }

    pub fn serialize<S>(datetime: &DateTime<FixedOffset>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let datetime_str = datetime.to_rfc3339();
        serializer.serialize_str(&datetime_str)
    }
}

pub fn to_metadata_entries(
    json: &DayOneExport,
    camera_profiles: Option<Vec<CameraLensProfile>>,
) -> Vec<MetadataEntryType> {
    let profiles = CameraProfileMap::new(camera_profiles);

    json.entries
        .iter()
        .map(|e| MetadataEntryType::new(e, &profiles))
        .collect()
}

impl MetadataEntryType {
    fn new(entry: &DayOneExportEntry, profiles: &CameraProfileMap) -> Self {
        let frame_count = parse_frame_count(&entry.text);
        if frame_count.is_empty() {
            Self::Leader(LeaderEntry::new(entry))
        } else {
            Self::Frame(FrameEntry::new(frame_count, entry, profiles))
        }
    }

    pub fn entry(&self) -> &MetadataEntry {
        match self {
            Self::Leader(leader) => &leader.entry,
            Self::Frame(frame) => &frame.entry,
        }
    }

    pub fn push_tag(&mut self, tag: String) {
        match self {
            Self::Leader(leader) => &leader.entry.tags.push(tag),
            Self::Frame(frame) => &frame.entry.tags.push(tag),
        };
    }
}

impl MetadataEntry {
    fn new(entry: &DayOneExportEntry) -> Self {
        let text = entry.text.clone();
        let entry_date = entry.creation_date;
        let mut tags = entry.tags.clone();
        let location = entry.location.clone();
        let maybe_weather_info = entry.weather.clone();

        let text = text.replace('\\', "");
        tags.sort();

        let mut entry = MetadataEntry {
            entry: entry.clone(),
            text,
            entry_date,
            location,
            tags,
        };
        if let Some(weather_info) = maybe_weather_info {
            entry.populate_weather_info_tags(&weather_info);
        }

        entry
    }

    fn fake(text: String, entry_date: DateTime<FixedOffset>) -> Self {
        Self {
            text,
            entry_date,
            ..Default::default()
        }
    }

    pub fn camera_tag(&self) -> Option<&String> {
        self.tags.iter().find(|t| t.starts_with("camera:"))
    }

    pub fn text(&self) -> &String {
        &self.text
    }

    pub fn entry_date(&self) -> &DateTime<FixedOffset> {
        &self.entry_date
    }

    pub fn tags(&self) -> &Vec<String> {
        &self.tags
    }

    fn populate_weather_info_tags(&mut self, weather_info: &DayOneWeather) {
        debug!(
            "entry_date: {}, sunrise: {}, sunset: {}",
            self.entry_date, weather_info.sunrise_date, weather_info.sunset_date
        );
        if self.entry_date > weather_info.sunset_date {
            // If more than 30 minutes after sunset, consider it "night"
            let timedelta = self.entry_date - weather_info.sunset_date;
            let timedelta_in_mins = timedelta.num_minutes();
            debug!("after sunset, timedelta: {timedelta} timedelta_in_mins: {timedelta_in_mins}");
            if timedelta_in_mins > 30 {
                self.tags.push("night".to_string());
            } else {
                self.tags.push("dusk".to_string());
            }
        } else if self.entry_date > weather_info.sunrise_date {
            // If 30 minutes or less before sunset, consider it "sunset"
            let sunset_timedelta = weather_info.sunset_date - self.entry_date;
            let sunset_timedelta_in_mins = sunset_timedelta.num_minutes();
            debug!(
                "after sunrise before sunset, sunset_timedelta_in_mins: {sunset_timedelta_in_mins}",
            );
            if sunset_timedelta_in_mins <= 30 {
                self.tags.push("sunset".to_string());
            }

            // If 30 minutes or less after sunrise, consider it "sunrise"
            let sunrise_timedelta = self.entry_date - weather_info.sunrise_date;
            let sunrise_timedelta_in_mins = sunrise_timedelta.num_minutes();
            debug!(
                "after sunrise before sunset, sunrise_timedelta_in_mins: {sunrise_timedelta_in_mins}",
            );
            if sunrise_timedelta_in_mins <= 30 {
                self.tags.push("sunrise".to_string());
            }
        } else {
            // If 30 minutes or less before sunrise, consider it "dawn", else night
            let sunrise_timedelta = weather_info.sunrise_date - self.entry_date;
            let sunrise_timedelta_in_mins = sunrise_timedelta.num_minutes();
            if sunrise_timedelta_in_mins > 30 {
                self.tags.push("night".to_string());
            } else {
                self.tags.push("dawn".to_string());
            }
        }
    }
}

impl LeaderEntry {
    pub fn fake(text: String, entry_date: DateTime<FixedOffset>, emulsion_name: String) -> Self {
        let entry = MetadataEntry::fake(text, entry_date);
        LeaderEntry {
            entry,
            emulsion_name,
            entry_date,
        }
    }

    fn new(entry: &DayOneExportEntry) -> Self {
        let entry = MetadataEntry::new(entry);
        let emulsion_name = LeaderEntry::extract_emulsion_name_from_leader_text(&entry.text);
        let entry_date = entry.entry_date;
        debug!(
            "Frame XX: Leader: Text: {} // Found emulsion: {emulsion_name}",
            &entry.text
        );

        LeaderEntry {
            entry,
            emulsion_name,
            entry_date,
        }
    }

    fn extract_emulsion_name_from_leader_text(text: &str) -> String {
        let leader_headline_text_matcher =
            Regex::new("# (.*) - ((120)|(35mm)) (\\(.*\\))").unwrap();
        if let Some(captures) = leader_headline_text_matcher.captures(text) {
            return captures.get(1).unwrap().as_str().to_string();
        }

        String::default()
    }

    pub fn emulsion(&self) -> String {
        self.emulsion_name.clone()
    }

    pub fn entry_date(&self) -> DateTime<FixedOffset> {
        self.entry_date.clone()
    }

}

fn calculate_aperture_apex_val(aperture_fstop: f32) -> i8 {
    (aperture_fstop.log2() * 2.0).round_ties_even() as i8
}

impl FrameEntry {
    pub fn fake(frame_count: String, text: String, entry_date: DateTime<FixedOffset>) -> Self {
        let entry = MetadataEntry::fake(text, entry_date);
        FrameEntry {
            entry,
            frame_count,
            exif_tags: Vec::new(),
            ..Default::default()
        }
    }

    fn new(frame_count: String, entry: &DayOneExportEntry, profiles: &CameraProfileMap) -> Self {
        let exif_tags = Vec::new();
        let entry = MetadataEntry::new(entry);
        let mut frame_entry = Self {
            entry,
            frame_count,
            exif_tags,
            ..Default::default()
        };
        frame_entry.populate_tags(profiles);

        frame_entry
    }

    pub fn frame_count(&self) -> &String {
        &self.frame_count
    }

    pub fn exif_tags(&self) -> &Vec<ExifTag> {
        &self.exif_tags
    }

    pub fn associated_camera(&self) -> Option<&String> {
        self.entry.tags.iter().find(|t| t.starts_with("camera:"))
    }

    fn populate_tags(&mut self, profiles: &CameraProfileMap) {
        let munged_datetime = self.munge_date_with_framecount();
        let munged_datetime_tag = munged_datetime.to_exif_tag("DateTimeOriginal");
        debug!(
            "Frame {}: Munging date/time to {munged_datetime}",
            self.frame_count
        );
        self.exif_tags.push(munged_datetime_tag);

        self.populate_caption_from_text();

        self.populate_location_tags();
        self.populate_from_entry_tags(profiles);
    }

    fn populate_caption_from_text(&mut self) {
        let mut text_lines = self.entry.text.lines();
        text_lines.next();
        let text_sans_header = text_lines.fold(String::new(), |mut a, b| {
            a.reserve(b.len() + 1);
            a.push('\n');
            a.push_str(b);
            a
        });

        if text_sans_header.is_empty() {
            debug!("Frame {}: No caption found", self.frame_count);
        } else {
            let text_tag = text_sans_header.to_exif_tag("UserComment");
            debug!(
                "Frame {}: Found caption: {text_sans_header}",
                self.frame_count
            );
            self.exif_tags.push(text_tag);
        }
    }

    fn populate_location_tags(&mut self) {
        if let Some(location) = &self.entry.location {
            if let Some(region) = &location.region {
                let lat = region.center.latitude;
                let lon = region.center.longitude;
                let radius = region.radius;

                let gps_lat_tag = lat.to_exif_tag("GPSLatitude");
                let gps_lon_tag = lon.to_exif_tag("GPSLongitude");
                let gps_hpos_error_tag = radius.to_exif_tag("GPSHPositioningError");

                self.exif_tags.push(gps_lat_tag);
                self.exif_tags.push(gps_lon_tag);
                self.exif_tags.push(gps_hpos_error_tag);
                debug!(
                    "Frame {}: Found GPS: lat {lat} lon {lon} hpos {radius}",
                    self.frame_count
                );
            }

            if let Some(country) = &location.country {
                let country_tag = country.to_exif_tag("Country");
                debug!("Frame {}: Found Country: {country}", self.frame_count);
                self.exif_tags.push(country_tag);
            }

            if let Some(admin_area) = &location.administrative_area {
                let admin_area_tag = admin_area.to_exif_tag("State");
                debug!("Frame {}: Found Admin Area: {admin_area}", self.frame_count);
                self.exif_tags.push(admin_area_tag);
            }
        }
    }

    fn populate_from_entry_tags(&mut self, profiles: &CameraProfileMap) {
        let shutter_tag_matcher = Regex::new("(1/)?\\d+s").unwrap();
        let lens_focal_length_matcher = Regex::new(r"(\d+)mm").unwrap();

        let mut found_camera_tag: Option<String> = None;
        let mut found_lens_tag: Option<String> = None;

        self.entry.tags.retain(|tag| {
            if shutter_tag_matcher.is_match(tag) {
                let shutter_speed = tag.strip_suffix('s').unwrap();
                self.exif_tags
                    .push(shutter_speed.to_exif_tag("ExposureTime"));
                debug!("Frame {}: Shutter speed: {shutter_speed}", self.frame_count);
                return false;
            }

            if let Some(aperture_tag) = tag.strip_prefix("f/") {
                if let Ok(f_number) = aperture_tag.parse::<f32>() {
                    self.exif_tags.push(f_number.to_exif_tag("FNumber"));
                    debug!("Frame {}: Aperture: {aperture_tag}", self.frame_count);
                    return false;
                }
            }

            if tag.starts_with("lens:") {
                found_lens_tag = Some(tag.clone());
                debug!("Frame {}: Found lens tag: {tag}", self.frame_count);
                if let Some(captures) = lens_focal_length_matcher.captures(tag) {
                    let focal_length = captures.get(1).unwrap().as_str().parse::<f64>().unwrap();
                    self.exif_tags.push(focal_length.to_exif_tag("FocalLength"));
                }
            }

            if tag.starts_with("camera:") {
                found_camera_tag = Some(tag.clone());
            }

            if tag == "unindexed" || tag == "scanned" {
                return false;
            }

            true
        });

        self.populate_from_camera_profile(profiles, found_camera_tag, found_lens_tag);

        // Replace film type tags (120, 135) with prefixed tags
        for tag in &mut self.entry.tags {
            if tag == "120" {
                *tag = "film:120".to_string();
            }
            if tag == "35mm" {
                *tag = "film:135".to_string();
            }
        }

        let keyword_tag = self.entry.tags.to_exif_tag("Keywords");
        self.exif_tags.push(keyword_tag);
    }

    fn populate_from_camera_profile(
        &mut self,
        profiles: &CameraProfileMap,
        camera_tag: Option<String>,
        lens_tag: Option<String>,
    ) {
        if let Some(profile_tuple) = profiles.get_profile(camera_tag, lens_tag) {
            let (camera_profile, lens_profile) = profile_tuple;
            debug!(
                "    : Found camera and lens profile: {}, {}",
                camera_profile.name, lens_profile.name
            );

            let min_focal_length =
                format!("{}mm", lens_profile.min_focal_length_mm).to_exif_tag("MinFocalLength");
            let max_focal_length =
                format!("{}mm", lens_profile.max_focal_length_mm).to_exif_tag("MaxFocalLength");
            let max_aperture_short_str = format!("f/{}", lens_profile.max_aperture_at_short);
            let max_aperture_long_str = format!("f/{}", lens_profile.max_aperture_at_long);

            let max_aperture_val = calculate_aperture_apex_val(lens_profile.max_aperture_at_short);
            let max_aperture = max_aperture_val.to_exif_tag("MaxAperture");
            let max_aperture_at_short = max_aperture_short_str.to_exif_tag("MaxApertureAtMinFocal");
            let max_aperture_at_long = max_aperture_long_str.to_exif_tag("MaxApertureAtMaxFocal");

            let lens_model = lens_profile.name.to_exif_tag("LensModel");
            let camera_label = camera_profile.name.to_exif_tag("CameraLabel");

            self.exif_tags.push(lens_model);
            self.exif_tags.push(camera_label);

            self.exif_tags.push(min_focal_length);
            self.exif_tags.push(max_focal_length);
            self.exif_tags.push(max_aperture);
            self.exif_tags.push(max_aperture_at_short);
            self.exif_tags.push(max_aperture_at_long);

            if let Some(exif_tags) = &lens_profile.exif_tags {
                for (tag, value) in exif_tags {
                    let exif_tag = value.to_exif_tag(tag);
                    self.exif_tags.push(exif_tag);
                }
            }
            if let Some(exif_tags) = &camera_profile.exif_tags {
                for (tag, value) in exif_tags {
                    let exif_tag = value.to_exif_tag(tag);
                    self.exif_tags.push(exif_tag);
                }
            }
        }
    }

    const EXIF_DATE_FORMAT: &str = "%Y:%m:%d %H:%M:%S";

    fn munge_date_with_framecount(&self) -> String {
        let frame_count_maybe_number = self.frame_count.parse::<u32>();
        let munged_datetime = if let Ok(frame_count_number) = frame_count_maybe_number {
            self.entry
                .entry_date
                .with_second(frame_count_number)
                .unwrap()
        } else {
            self.entry.entry_date
        };

        let local_tz = *Local::now().offset();
        if let Some(location) = &self.entry.location {
            if let Some(tz_name) = &location.time_zone_name {
                let maybe_tz = tz_name.parse::<Tz>();
                let formatted_munged_datetime = match maybe_tz {
                    Ok(tz) => {
                        debug!("TZ => {tz}");
                        munged_datetime
                            .with_timezone(&tz)
                            .format(Self::EXIF_DATE_FORMAT)
                    }
                    Err(e) => {
                        warn!("Couldn't find '{tz_name}', falling back to local: {e}");
                        munged_datetime
                            .with_timezone(&local_tz)
                            .format(Self::EXIF_DATE_FORMAT)
                    }
                };
                return format!("{formatted_munged_datetime}");
            }
        }

        format!(
            "{}",
            munged_datetime
                .with_timezone(&local_tz)
                .format(Self::EXIF_DATE_FORMAT)
        )
    }
}

impl<'a> Roll<'a> {
    pub fn new(
        start_date: DateTime<FixedOffset>,
        camera: String,
        emulsion: String,
        entries: Vec<&'a MetadataEntryType>,
    ) -> Self {
        Self {
            camera,
            emulsion,
            entries,
            start_date,
        }
    }

    pub fn entries(&self) -> &Vec<&'a MetadataEntryType> {
        &self.entries
    }

    fn to_dayone_export(&self) -> DayOneExport {
        DayOneExport {
            metadata: DayOneExportMetadata {
                version: "1.0".to_string(),
            },
            entries: self
                .entries
                .iter()
                .map(|e| e.entry().entry.clone())
                .collect(),
        }
    }

    // emulsion_camera_idx
    fn cons_filename(&self, idx: usize) -> String {
        let emulsion = self
            .emulsion
            .replace(' ', "_")
            .replace('@', "-")
            .to_lowercase();
        let camera = self.camera.replace("camera:", "").to_lowercase();
        let start_date = format!("{}", self.start_date.format("%Y%m%d"));

        format!("{emulsion}_{camera}_{start_date}_{idx}.zip")
    }

    pub fn serialize_to(
        &self,
        idx: usize,
        output_directory: &Path,
        overwrite: bool,
    ) -> Result<(), MetadataWriteError> {
        let filename = self.cons_filename(idx);
        let mut output_filename = output_directory.to_path_buf();
        output_filename.push(filename);

        debug!(
            "Serializing to {}: Camera: {}, Emulsion {}, Entries (count) {}",
            output_filename.display(),
            self.camera,
            self.emulsion,
            self.entries.len()
        );

        if output_filename.exists() && overwrite {
            debug!("Removing existing file: {}", output_filename.display());
            remove_file(&output_filename)?;
        }

        let writer = File::create_new(&output_filename)?;
        let mut zipfile = ZipWriter::new(writer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zipfile.start_file("Journal.json", options)?;
        let export_root = self.to_dayone_export();
        serde_json::to_writer_pretty(zipfile, &export_root)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::exif::TagValue;
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn should_calculate_correct_aperture_apex_val() {
        assert_eq!(0, calculate_aperture_apex_val(1.0));
        assert_eq!(2, calculate_aperture_apex_val(2.0));
        assert_eq!(5, calculate_aperture_apex_val(5.6));
    }

    #[test]
    fn should_munge_datetime_from_export() {
        let metadata = FrameEntry {
            entry: MetadataEntry {
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: Some(DayOneLocation {
                    region: Some(DayOneRegion {
                        center: LongLat {
                            longitude: -12.34,
                            latitude: -56.78,
                        },
                        radius: 0.0,
                    }),
                    country: Some("Country".to_string()),
                    administrative_area: Some("AdminArea".to_string()),
                    time_zone_name: Some("UTC".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };

        assert_eq!("2025:01:02 03:04:59", metadata.munge_date_with_framecount());
    }

    #[test]
    fn should_munge_datetime_from_export_with_given_timezone() {
        let metadata = FrameEntry {
            entry: MetadataEntry {
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: Some(DayOneLocation {
                    region: Some(DayOneRegion {
                        center: LongLat {
                            longitude: -12.34,
                            latitude: -56.78,
                        },
                        radius: 0.0,
                    }),
                    country: Some("Country".to_string()),
                    administrative_area: Some("AdminArea".to_string()),
                    time_zone_name: Some("Australia/Sydney".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };

        assert_eq!("2025:01:02 14:04:59", metadata.munge_date_with_framecount());
    }

    #[test]
    fn should_munge_datetime_from_export_falling_back_to_local() {
        let metadata = FrameEntry {
            entry: MetadataEntry {
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: Some(DayOneLocation {
                    region: Some(DayOneRegion {
                        center: LongLat {
                            longitude: -12.34,
                            latitude: -56.78,
                        },
                        radius: 0.0,
                    }),
                    country: Some("Country".to_string()),
                    administrative_area: Some("AdminArea".to_string()),
                    time_zone_name: Some("Australia/Foo".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };

        let local_tz = *Local::now().offset();
        let expected_munged_entry_date = Utc
            .with_ymd_and_hms(2025, 1, 2, 3, 4, 59)
            .unwrap()
            .with_timezone(&local_tz);
        assert_eq!(
            format!(
                "{}",
                expected_munged_entry_date.format(FrameEntry::EXIF_DATE_FORMAT)
            ),
            metadata.munge_date_with_framecount()
        );
    }

    #[test]
    fn should_populate_location_tags() {
        let mut metadata = FrameEntry {
            entry: MetadataEntry {
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: Some(DayOneLocation {
                    region: Some(DayOneRegion {
                        center: LongLat {
                            longitude: -12.34,
                            latitude: -56.78,
                        },
                        radius: 0.0,
                    }),
                    country: Some("Country".to_string()),
                    administrative_area: Some("AdminArea".to_string()),
                    time_zone_name: Some("Australia/Foo".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };
        metadata.populate_location_tags();

        assert!(
            metadata
                .exif_tags
                .contains(&(-56.78).to_exif_tag("GPSLatitude"))
        );
        assert!(
            metadata
                .exif_tags
                .contains(&(-12.34).to_exif_tag("GPSLongitude"))
        );
        assert!(
            metadata
                .exif_tags
                .contains(&"Country".to_exif_tag("Country"))
        );
        assert!(
            metadata
                .exif_tags
                .contains(&"AdminArea".to_exif_tag("State"))
        );
    }

    #[test]
    fn should_populate_from_entry_tags() {
        let tags = vec![
            "lens:35mm".to_string(),
            "camera:canonp".to_string(),
            "f/2".to_string(),
            "1/500s".to_string(),
            "unindexed".to_string(),
            "scanned".to_string(),
        ];
        let profiles = CameraProfileMap::default();
        let mut metadata = FrameEntry {
            entry: MetadataEntry {
                entry: DayOneExportEntry::default(),
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: None,
                tags,
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };
        metadata.populate_from_entry_tags(&profiles);
        assert!(!metadata.entry.tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry.tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry.tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry.tags.contains(&"1/500s".to_string()));

        let tag_map: HashMap<_, _> = metadata
            .exif_tags
            .iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        assert!(tag_map.contains_key("ExposureTime"));
        assert_eq!(
            TagValue::String("1/500".to_string()),
            tag_map.get("ExposureTime").unwrap().value
        );
        assert!(tag_map.contains_key("FNumber"));
        assert_eq!(TagValue::Float(2.0), tag_map.get("FNumber").unwrap().value);
        assert!(tag_map.contains_key("FocalLength"));
        assert_eq!(
            TagValue::Float(35.0),
            tag_map.get("FocalLength").unwrap().value
        );
    }

    #[test]
    fn should_populate_weather_info_tags_dawn() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T18:34:56Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"dawn".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_dusk() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"dusk".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_sunrise() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T19:29:00Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"sunrise".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_sunset() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:00:56Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"sunset".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_night_after_sunset() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T08:01:00Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"night".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_night_before_sunrise() {
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T18:00:00Z").unwrap(),
            location: None,
            tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert_eq!(1, entry.tags.len());
        assert!(entry.tags.contains(&"night".to_string()))
    }

    #[test]
    fn should_include_day_keywords_implied_from_weather() {
        let tags = vec![
            "lens:35mm".to_string(),
            "camera:canonp".to_string(),
            "f/2".to_string(),
            "1/500s".to_string(),
            "unindexed".to_string(),
            "scanned".to_string(),
        ];
        let mut entry = MetadataEntry {
            entry: DayOneExportEntry::default(),
            text: "# 1 // Some raw text\nSome body".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
            location: None,
            tags,
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
            ..Default::default()
        };
        entry.populate_weather_info_tags(&weather_info);
        assert!(entry.tags.contains(&"unindexed".to_string()));
        assert!(entry.tags.contains(&"scanned".to_string()));
        assert!(entry.tags.contains(&"f/2".to_string()));
        assert!(entry.tags.contains(&"1/500s".to_string()));

        assert!(entry.tags.contains(&"dusk".to_string()));
    }

    #[test]
    fn when_populate_from_entry_tags_should_ignore_invalid_aperture() {
        let tags = vec![
            "lens:35mm".to_string(),
            "camera:canonp".to_string(),
            "f/a".to_string(),
            "1/500s".to_string(),
            "unindexed".to_string(),
            "scanned".to_string(),
        ];
        let profiles = CameraProfileMap::default();
        let mut metadata = FrameEntry {
            entry: MetadataEntry {
                entry: DayOneExportEntry::default(),
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: None,
                tags,
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };
        metadata.populate_from_entry_tags(&profiles);
        assert!(!metadata.entry.tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry.tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry.tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry.tags.contains(&"1/500s".to_string()));

        let tag_map: HashMap<_, _> = metadata
            .exif_tags
            .iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        assert!(tag_map.contains_key("ExposureTime"));
        assert_eq!(
            TagValue::String("1/500".to_string()),
            tag_map.get("ExposureTime").unwrap().value
        );
        assert!(!tag_map.contains_key("FNumber"));

        assert!(tag_map.contains_key("FocalLength"));
        assert_eq!(
            TagValue::Float(35.0),
            tag_map.get("FocalLength").unwrap().value
        );
    }

    #[test]
    fn when_populate_from_entry_tags_should_handle_unconventional_lens_tag() {
        let tags = vec![
            "lens:50mm1.4-AiS".to_string(),
            "camera:canonp".to_string(),
            "f/2".to_string(),
            "1/500s".to_string(),
            "unindexed".to_string(),
            "scanned".to_string(),
        ];
        let profiles = CameraProfileMap::default();
        let mut metadata = FrameEntry {
            entry: MetadataEntry {
                entry: DayOneExportEntry::default(),
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: None,
                tags,
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };
        metadata.populate_from_entry_tags(&profiles);
        assert!(!metadata.entry.tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry.tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry.tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry.tags.contains(&"1/500s".to_string()));

        let tag_map: HashMap<_, _> = metadata
            .exif_tags
            .iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        assert!(tag_map.contains_key("ExposureTime"));
        assert_eq!(
            TagValue::String("1/500".to_string()),
            tag_map.get("ExposureTime").unwrap().value
        );
        assert!(tag_map.contains_key("FNumber"));
        assert_eq!(TagValue::Float(2.0), tag_map.get("FNumber").unwrap().value);
        assert!(tag_map.contains_key("FocalLength"));
        assert_eq!(
            TagValue::Float(50.0),
            tag_map.get("FocalLength").unwrap().value
        );
    }

    #[test]
    fn when_populate_from_entry_tags_should_transform_film_type_tags() {
        let tags = vec!["120".to_string(), "35mm".to_string()];
        let profiles = CameraProfileMap::default();
        let mut metadata = FrameEntry {
            entry: MetadataEntry {
                entry: DayOneExportEntry::default(),
                text: "# 1 // Some raw text\nSome body".to_string(),
                entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
                location: None,
                tags,
            },
            frame_count: "59".to_string(),
            exif_tags: Vec::new(),
            ..Default::default()
        };
        metadata.populate_from_entry_tags(&profiles);
        assert!(!metadata.entry.tags.contains(&"35mm".to_string()));
        assert!(metadata.entry.tags.contains(&"film:135".to_string()));

        assert!(!metadata.entry.tags.contains(&"120".to_string()));
        assert!(metadata.entry.tags.contains(&"film:120".to_string()));
    }

    #[test]
    fn should_create_metadata_entry_given_entry_from_export() {
        let loc = DayOneLocation {
            region: Some(DayOneRegion {
                center: LongLat {
                    longitude: -12.34,
                    latitude: -56.78,
                },
                radius: 0.0,
            }),
            country: Some("Country".to_string()),
            administrative_area: Some("AdminArea".to_string()),
            time_zone_name: Some("UTC".to_string()),
            ..Default::default()
        };
        let profiles = CameraProfileMap::default();
        let entry = DayOneExportEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            location: Some(loc),
            tags: vec![
                "APs".to_string(),
                "f/8".to_string(),
                "lens:50mm".to_string(),
                "camera:fm3a".to_string(),
            ],
            creation_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            weather: None,
            ..Default::default()
        };
        let metadata = MetadataEntryType::new(&entry, &profiles);

        assert!(matches!(metadata, MetadataEntryType::Frame(_)));
        if let MetadataEntryType::Frame(metadata) = metadata {
            let result_exif_tags = metadata.exif_tags();
            let tag_map: HashMap<_, _> = result_exif_tags
                .iter()
                .map(|t| (t.name.clone(), t))
                .collect();

            assert!(tag_map.contains_key("DateTimeOriginal"));
            assert_eq!(
                TagValue::String("2025:01:02 03:04:01".to_string()),
                tag_map.get("DateTimeOriginal").unwrap().value
            );
        }
    }

    #[test]
    fn should_handle_creating_metadata_entry_on_leader_entry() {
        let loc = DayOneLocation {
            region: Some(DayOneRegion {
                center: LongLat {
                    longitude: -12.34,
                    latitude: -56.78,
                },
                radius: 0.0,
            }),
            country: Some("Country".to_string()),
            administrative_area: Some("AdminArea".to_string()),
            time_zone_name: Some("UTC".to_string()),
            ..Default::default()
        };
        let profiles = CameraProfileMap::default();
        let entry = DayOneExportEntry {
            text: "# HP5 Plus @ 1600 - 35mm (Canon P)".to_string(),
            creation_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            location: Some(loc),
            tags: Vec::default(),
            weather: None,
            ..Default::default()
        };
        let metadata = MetadataEntryType::new(&entry, &profiles);

        assert!(matches!(metadata, MetadataEntryType::Leader(_)));
    }

    #[test]
    fn should_capture_emulsion_name_on_new_leader_entry() {
        let loc = DayOneLocation {
            region: Some(DayOneRegion {
                center: LongLat {
                    longitude: -12.34,
                    latitude: -56.78,
                },
                radius: 0.0,
            }),
            country: Some("Country".to_string()),
            administrative_area: Some("AdminArea".to_string()),
            time_zone_name: Some("UTC".to_string()),
            ..Default::default()
        };
        let entry = DayOneExportEntry {
            text: "# HP5 Plus @ 1600 - 35mm (Canon P)".to_string(),
            creation_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            tags: Vec::new(),
            location: Some(loc),
            weather: None,
            ..Default::default()
        };
        let leader = LeaderEntry::new(&entry);

        assert_eq!("HP5 Plus @ 1600".to_owned(), leader.emulsion_name);
    }

    #[test]
    fn should_generate_acceptable_filename_from_roll_info() {
        let roll = Roll {
            camera: "camera:canonp".to_string(),
            emulsion: "HP5 Plus @ 1600".to_string(),
            entries: Vec::default(),
            start_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
        };

        assert_eq!(
            "hp5_plus_-_1600_canonp_20250102_1.zip",
            roll.cons_filename(1)
        )
    }
}
