use chrono::{DateTime, Local, Timelike};
use log::debug;
use regex::Regex;
use serde::Deserialize;

use std::collections::HashMap;

use crate::exif::{ExifTag, ExifTagTrait};

#[derive(Deserialize)]
pub struct DayOneExportMetadata {
    pub version: String,
}

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
    pub metadata: DayOneExportMetadata,
    entries: Vec<DayOneExportEntry>,
}

#[derive(Debug)]
pub struct MetadataEntry {
    frame_count: String,
    text: String,
    entry_date: String,
    location: Option<DayOneLocation>,
    entry_tags: Vec<String>,
    exif_tags: Vec<ExifTag>,
}

fn parse_frame_count(text: &str) -> String {
    text.lines()
        .next()
        .and_then(|header| header.split_whitespace().nth(1))
        .unwrap_or("0")
        .parse()
        .unwrap()
}

pub fn to_metadata_entries(
    json: DayOneExport,
    camera_profiles: Option<Vec<CameraLensProfile>>,
) -> Vec<MetadataEntry> {
    let profiles = CameraProfileMap::new(camera_profiles);

    json.entries
        .iter()
        .map(|e| {
            let mut entry = MetadataEntry::new(
                e.text.clone(),
                e.creation_date.clone(),
                e.location.clone(),
                e.tags.clone(),
            );
            entry.populate_tags(&profiles);
            entry
        })
        .collect()
}

fn calculate_aperture_apex_val(aperture_fstop: f32) -> i8 {
    (aperture_fstop.log2() * 2.0).round_ties_even() as i8
}

impl MetadataEntry {
    fn new(
        raw_text: String,
        entry_date: String,
        location: Option<DayOneLocation>,
        raw_entry_tags: Vec<String>,
    ) -> Self {
        let exif_tags = Vec::new();
        let mut entry_tags = raw_entry_tags.clone();
        let text = raw_text.clone();
        let frame_count = parse_frame_count(&text);
        entry_tags.sort();
        Self {
            frame_count,
            text,
            entry_date,
            location,
            entry_tags,
            exif_tags,
        }
    }

    pub fn frame_count(&self) -> &String {
        &self.frame_count
    }

    pub fn exif_tags(&self) -> &Vec<ExifTag> {
        &self.exif_tags
    }

    pub fn populate_tags(&mut self, profiles: &CameraProfileMap) {
        let munged_datetime = self.munge_date_with_framecount();
        let munged_datetime_tag = munged_datetime.to_exif_tag("DateTimeOriginal");
        debug!(
            "Frame {}: Munging date/time to {}",
            self.frame_count, munged_datetime
        );
        self.exif_tags.push(munged_datetime_tag);

        self.populate_caption_from_text();

        self.populate_location_tags();
        self.populate_from_entry_tags(profiles);
    }

    fn populate_caption_from_text(&mut self) {
        let mut text_lines = self.text.lines();
        text_lines.next();
        let text_sans_header = text_lines.fold(String::new(), |mut a, b| {
            a.reserve(b.len() + 1);
            a.push('\n');
            a.push_str(&b.replace("\\", ""));
            a
        });

        let text_tag = text_sans_header.to_exif_tag("UserComment");
        debug!(
            "Frame {}: Found caption: {}",
            self.frame_count, text_sans_header
        );
        self.exif_tags.push(text_tag);
    }

    fn populate_location_tags(&mut self) {
        if let Some(location) = &self.location {
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
                    "Frame {}: Found GPS: lat {} lon {} hpos {}",
                    self.frame_count, lat, lon, radius
                );
            }

            if let Some(country) = &location.country {
                let country_tag = country.to_exif_tag("Country");
                debug!("Frame {}: Found Country: {}", self.frame_count, country);
                self.exif_tags.push(country_tag);
            }

            if let Some(admin_area) = &location.administrative_area {
                let admin_area_tag = admin_area.to_exif_tag("State");
                debug!(
                    "Frame {}: Found Admin Area: {}",
                    self.frame_count, admin_area
                );
                self.exif_tags.push(admin_area_tag);
            }
        }
    }

    fn populate_from_entry_tags(&mut self, profiles: &CameraProfileMap) {
        let shutter_tag_matcher = Regex::new("(1/)?\\d+s").unwrap();
        let lens_focal_length_matcher = Regex::new(r"(\d+)mm").unwrap();

        let mut found_camera_tag: Option<String> = None;
        let mut found_lens_tag: Option<String> = None;

        self.entry_tags.retain(|tag| {
            if shutter_tag_matcher.is_match(tag) {
                let shutter_speed = tag.strip_suffix('s').unwrap();
                self.exif_tags
                    .push(shutter_speed.to_exif_tag("ExposureTime"));
                debug!(
                    "Frame {}: Shutter speed: {}",
                    self.frame_count, shutter_speed
                );
                return false;
            }

            if let Some(aperture_tag) = tag.strip_prefix("f/") {
                if let Ok(f_number) = aperture_tag.parse::<f32>() {
                    self.exif_tags.push(f_number.to_exif_tag("FNumber"));
                    debug!("Frame {}: Aperture: {}", self.frame_count, aperture_tag);
                    return false;
                }
            }

            if tag.starts_with("lens:") {
                found_lens_tag = Some(tag.clone());
                debug!("Frame {}: Found lens tag: {}", self.frame_count, tag);
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
                for (tag, value) in exif_tags.iter() {
                    let exif_tag = value.to_exif_tag(tag);
                    self.exif_tags.push(exif_tag);
                }
            }
            if let Some(exif_tags) = &camera_profile.exif_tags {
                for (tag, value) in exif_tags.iter() {
                    let exif_tag = value.to_exif_tag(tag);
                    self.exif_tags.push(exif_tag);
                }
            }
        }
    }

    const EXIF_DATE_FORMAT: &str = "%Y:%m:%d %H:%M:%S";
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
                return format!(
                    "{}",
                    munged_datetime
                        .with_timezone(&tz)
                        .format(Self::EXIF_DATE_FORMAT)
                );
            }
        }

        format!(
            "{}",
            munged_datetime
                .with_timezone(&Local::now().offset().clone())
                .format(Self::EXIF_DATE_FORMAT)
        )
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct CameraLensProfile {
    name: String,
    tag: String,
    exif_tags: Option<HashMap<String, String>>,
    lens_profiles: Vec<LensProfile>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct LensProfile {
    name: String,
    tag: String,
    min_focal_length_mm: u16,
    max_focal_length_mm: u16,
    // min_aperture: f32,
    max_aperture_at_short: f32,
    max_aperture_at_long: f32,

    exif_tags: Option<HashMap<String, String>>,
}

#[derive(Debug)]
struct CameraProfileMapEntry {
    name: String,
    lens_profiles: HashMap<String, LensProfile>,
    exif_tags: Option<HashMap<String, String>>,
}

impl From<&CameraLensProfile> for CameraProfileMapEntry {
    fn from(item: &CameraLensProfile) -> Self {
        let lens_profiles_map: HashMap<_, _> = item
            .lens_profiles
            .iter()
            .map(|p| (p.tag.clone(), p.clone()))
            .collect();
        CameraProfileMapEntry {
            name: item.name.clone(),
            lens_profiles: lens_profiles_map,
            exif_tags: item.exif_tags.clone(),
        }
    }
}

pub struct CameraProfileMap {
    cameras: HashMap<String, CameraProfileMapEntry>,
}

impl CameraProfileMap {
    fn new(maybe_camera_profiles: Option<Vec<CameraLensProfile>>) -> Self {
        match maybe_camera_profiles {
            Some(camera_profiles) => {
                let cameras: HashMap<_, _> = camera_profiles
                    .iter()
                    .map(|p| (p.tag.clone(), p.into()))
                    .collect();
                debug!("Loaded camera profiles: {:#?}", cameras);
                CameraProfileMap { cameras }
            }
            None => CameraProfileMap {
                cameras: HashMap::new(),
            },
        }
    }

    fn get_profile(
        &self,
        maybe_camera_tag: Option<String>,
        maybe_lens_tag: Option<String>,
    ) -> Option<(&CameraProfileMapEntry, &LensProfile)> {
        if let (Some(camera_tag), Some(lens_tag)) = (maybe_camera_tag, maybe_lens_tag) {
            if let Some(camera_profile) = self.cameras.get(&camera_tag) {
                debug!(
                    "    : Looking up lens profile for camera {}, {}",
                    camera_tag, lens_tag
                );
                camera_profile
                    .lens_profiles
                    .get(&lens_tag)
                    .map(|lens_profile| (camera_profile, lens_profile))
            } else {
                debug!("    : No matching camera profile: {}", camera_tag);
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_calculate_correct_aperture_apex_val() {
        assert_eq!(0, calculate_aperture_apex_val(1.0));
        assert_eq!(2, calculate_aperture_apex_val(2.0));
        assert_eq!(5, calculate_aperture_apex_val(5.6));
    }
}
