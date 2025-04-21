use chrono::{DateTime, FixedOffset, Local, Timelike};
use chrono_tz::Tz;
use log::{debug, warn};
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
struct DayOneWeather {
    #[serde(rename = "sunriseDate", with = "json_date")]
    sunrise_date: DateTime<FixedOffset>,
    #[serde(rename = "sunsetDate", with = "json_date")]
    sunset_date: DateTime<FixedOffset>,
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
    #[serde(rename = "creationDate", with = "json_date")]
    creation_date: DateTime<FixedOffset>,
    text: String,
    weather: Option<DayOneWeather>,
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
    entry_date: DateTime<FixedOffset>,
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

mod json_date {
    use chrono::{DateTime, FixedOffset};
    use serde::{self, Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&s).map_err(serde::de::Error::custom)
    }
}

pub fn to_metadata_entries(
    json: DayOneExport,
    camera_profiles: Option<Vec<CameraLensProfile>>,
) -> Vec<MetadataEntry> {
    let profiles = CameraProfileMap::new(camera_profiles);

    json.entries
        .iter()
        .map(|e| {
            MetadataEntry::new(
                e.text.clone(),
                e.creation_date,
                e.location.clone(),
                e.tags.clone(),
                e.weather.clone(),
                &profiles,
            )
        })
        .collect()
}

fn calculate_aperture_apex_val(aperture_fstop: f32) -> i8 {
    (aperture_fstop.log2() * 2.0).round_ties_even() as i8
}

impl MetadataEntry {
    pub fn fake(frame_count: String, text: String, entry_date: DateTime<FixedOffset>) -> Self {
        MetadataEntry {
            frame_count,
            text,
            entry_date,
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        }
    }

    fn new(
        raw_text: String,
        entry_date: DateTime<FixedOffset>,
        location: Option<DayOneLocation>,
        raw_entry_tags: Vec<String>,
        maybe_weather_info: Option<DayOneWeather>,
        profiles: &CameraProfileMap,
    ) -> Self {
        let exif_tags = Vec::new();
        let mut entry_tags = raw_entry_tags.clone();
        let text = raw_text.clone();
        let frame_count = parse_frame_count(&text);
        entry_tags.sort();
        let mut entry = Self {
            frame_count,
            text,
            entry_date,
            location,
            entry_tags,
            exif_tags,
        };
        entry.populate_tags(maybe_weather_info, profiles);

        entry
    }

    pub fn frame_count(&self) -> &String {
        &self.frame_count
    }

    pub fn exif_tags(&self) -> &Vec<ExifTag> {
        &self.exif_tags
    }

    fn populate_tags(
        &mut self,
        maybe_weather_info: Option<DayOneWeather>,
        profiles: &CameraProfileMap,
    ) {
        let munged_datetime = self.munge_date_with_framecount();
        let munged_datetime_tag = munged_datetime.to_exif_tag("DateTimeOriginal");
        debug!(
            "Frame {}: Munging date/time to {}",
            self.frame_count, munged_datetime
        );
        self.exif_tags.push(munged_datetime_tag);

        self.populate_caption_from_text();

        self.populate_location_tags();
        self.populate_from_entry_tags(maybe_weather_info, profiles);
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

    fn populate_from_entry_tags(
        &mut self,
        maybe_weather_info: Option<DayOneWeather>,
        profiles: &CameraProfileMap,
    ) {
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

        if let Some(weather_info) = maybe_weather_info {
            self.populate_weather_info_tags(weather_info);
        }

        let keyword_tag = self.entry_tags.to_exif_tag("Keywords");
        self.exif_tags.push(keyword_tag);
    }

    fn populate_weather_info_tags(&mut self, weather_info: DayOneWeather) {
        debug!(
            "entry_date: {}, sunrise: {}, sunset: {}",
            self.entry_date, weather_info.sunrise_date, weather_info.sunset_date
        );
        if self.entry_date > weather_info.sunset_date {
            // If more than 30 minutes after sunset, consider it "night"
            let timedelta = self.entry_date - weather_info.sunset_date;
            let timedelta_in_mins = timedelta.num_minutes();
            debug!(
                "after sunset, timedelta: {} timedelta_in_mins: {}",
                timedelta, timedelta_in_mins
            );
            if timedelta_in_mins > 30 {
                self.entry_tags.push("night".to_string());
            } else {
                self.entry_tags.push("dusk".to_string());
            }
        } else if self.entry_date > weather_info.sunrise_date {
            // If 30 minutes or less before sunset, consider it "sunset"
            let sunset_timedelta = weather_info.sunset_date - self.entry_date;
            let sunset_timedelta_in_mins = sunset_timedelta.num_minutes();
            debug!(
                "after sunrise before sunset, sunset_timedelta_in_mins: {}",
                sunset_timedelta_in_mins
            );
            if sunset_timedelta_in_mins <= 30 {
                self.entry_tags.push("sunset".to_string());
            }

            // If 30 minutes or less after sunrise, consider it "sunrise"
            let sunrise_timedelta = self.entry_date - weather_info.sunrise_date;
            let sunrise_timedelta_in_mins = sunrise_timedelta.num_minutes();
            debug!(
                "after sunrise before sunset, sunrise_timedelta_in_mins: {}",
                sunrise_timedelta_in_mins
            );
            if sunrise_timedelta_in_mins <= 30 {
                self.entry_tags.push("sunrise".to_string());
            }
        } else {
            // If 30 minutes or less before sunrise, consider it "dawn", else night
            let sunrise_timedelta = weather_info.sunrise_date - self.entry_date;
            let sunrise_timedelta_in_mins = sunrise_timedelta.num_minutes();
            if sunrise_timedelta_in_mins > 30 {
                self.entry_tags.push("night".to_string());
            } else {
                self.entry_tags.push("dawn".to_string());
            }
        }
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

    fn munge_date_with_framecount(&self) -> String {
        let munged_datetime = self
            .entry_date
            .with_second(self.frame_count.parse::<u32>().unwrap())
            .unwrap();
        let local_tz = *Local::now().offset();
        if let Some(location) = &self.location {
            if let Some(tz_name) = &location.time_zone_name {
                let maybe_tz = tz_name.parse::<Tz>();
                let formatted_munged_datetime = match maybe_tz {
                    Ok(tz) => {
                        debug!("TZ => {}", tz);
                        munged_datetime
                            .with_timezone(&tz)
                            .format(Self::EXIF_DATE_FORMAT)
                    }
                    Err(e) => {
                        warn!("Couldn't find '{}', falling back to local: {}", tz_name, e);
                        munged_datetime
                            .with_timezone(&local_tz)
                            .format(Self::EXIF_DATE_FORMAT)
                    }
                };
                return format!("{}", formatted_munged_datetime);
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
    pub fn empty() -> Self {
        CameraProfileMap {
            cameras: HashMap::new(),
        }
    }

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
    use chrono::{TimeZone, Utc};

    use crate::exif::TagValue;

    use super::*;

    #[test]
    fn should_calculate_correct_aperture_apex_val() {
        assert_eq!(0, calculate_aperture_apex_val(1.0));
        assert_eq!(2, calculate_aperture_apex_val(2.0));
        assert_eq!(5, calculate_aperture_apex_val(5.6));
    }

    #[test]
    fn should_munge_datetime_from_export() {
        let metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
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
            }),
            entry_tags: Vec::default(),
            exif_tags: Vec::new(),
        };

        assert_eq!("2025:01:02 03:04:59", metadata.munge_date_with_framecount());
    }

    #[test]
    fn should_munge_datetime_from_export_with_given_timezone() {
        let metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
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
            }),
            entry_tags: Vec::default(),
            exif_tags: Vec::new(),
        };

        assert_eq!("2025:01:02 14:04:59", metadata.munge_date_with_framecount());
    }

    #[test]
    fn should_munge_datetime_from_export_falling_back_to_local() {
        let metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
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
            }),
            entry_tags: Vec::default(),
            exif_tags: Vec::new(),
        };

        let local_tz = *Local::now().offset();
        let expected_munged_entry_date = Utc
            .with_ymd_and_hms(2025, 1, 2, 3, 4, 59)
            .unwrap()
            .with_timezone(&local_tz);
        assert_eq!(
            format!(
                "{}",
                expected_munged_entry_date.format(MetadataEntry::EXIF_DATE_FORMAT)
            ),
            metadata.munge_date_with_framecount()
        );
    }

    #[test]
    fn should_populate_location_tags() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
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
            }),
            entry_tags: Vec::default(),
            exif_tags: Vec::new(),
        };
        metadata.populate_location_tags();

        assert!(metadata
            .exif_tags
            .contains(&(-56.78).to_exif_tag("GPSLatitude")));
        assert!(metadata
            .exif_tags
            .contains(&(-12.34).to_exif_tag("GPSLongitude")));
        assert!(metadata
            .exif_tags
            .contains(&"Country".to_exif_tag("Country")));
        assert!(metadata
            .exif_tags
            .contains(&"AdminArea".to_exif_tag("State")));
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
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            location: None,
            entry_tags: tags,
            exif_tags: Vec::new(),
        };
        metadata.populate_from_entry_tags(None, &profiles);
        assert!(!metadata.entry_tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry_tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry_tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry_tags.contains(&"1/500s".to_string()));

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
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T18:34:56Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"dawn".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_dusk() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"dusk".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_sunrise() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T19:29:00Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"sunrise".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_sunset() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:00:56Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"sunset".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_night_after_sunset() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T08:01:00Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"night".to_string()))
    }

    #[test]
    fn should_populate_weather_info_tags_night_before_sunrise() {
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-01T18:00:00Z").unwrap(),
            location: None,
            entry_tags: Vec::new(),
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_weather_info_tags(weather_info);
        assert_eq!(1, metadata.entry_tags.len());
        assert!(metadata.entry_tags.contains(&"night".to_string()))
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
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T07:34:56Z").unwrap(),
            location: None,
            entry_tags: tags,
            exif_tags: Vec::new(),
        };
        let weather_info = DayOneWeather {
            sunrise_date: DateTime::parse_from_rfc3339("2025-01-01T19:00:00Z").unwrap(),
            sunset_date: DateTime::parse_from_rfc3339("2025-01-02T07:30:00Z").unwrap(),
        };
        metadata.populate_from_entry_tags(Some(weather_info), &profiles);
        assert!(!metadata.entry_tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry_tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry_tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry_tags.contains(&"1/500s".to_string()));

        assert!(metadata.entry_tags.contains(&"dusk".to_string()));
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
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            location: None,
            entry_tags: tags,
            exif_tags: Vec::new(),
        };
        metadata.populate_from_entry_tags(None, &profiles);
        assert!(!metadata.entry_tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry_tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry_tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry_tags.contains(&"1/500s".to_string()));

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
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            location: None,
            entry_tags: tags,
            exif_tags: Vec::new(),
        };
        metadata.populate_from_entry_tags(None, &profiles);
        assert!(!metadata.entry_tags.contains(&"unindexed".to_string()));
        assert!(!metadata.entry_tags.contains(&"scanned".to_string()));
        assert!(!metadata.entry_tags.contains(&"f/2".to_string()));
        assert!(!metadata.entry_tags.contains(&"1/500s".to_string()));

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
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };
        let mut metadata = MetadataEntry {
            text: "# 1 // Some raw text\nSome body".to_string(),
            frame_count: "59".to_string(),
            entry_date: DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            location: None,
            entry_tags: tags,
            exif_tags: Vec::new(),
        };
        metadata.populate_from_entry_tags(None, &profiles);
        assert!(!metadata.entry_tags.contains(&"35mm".to_string()));
        assert!(metadata.entry_tags.contains(&"film:135".to_string()));

        assert!(!metadata.entry_tags.contains(&"120".to_string()));
        assert!(metadata.entry_tags.contains(&"film:120".to_string()));
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
        };
        let profiles = CameraProfileMap {
            cameras: HashMap::default(),
        };

        let metadata = MetadataEntry::new(
            "# 1 // Some raw text\nSome body".to_string(),
            DateTime::parse_from_rfc3339("2025-01-02T03:04:56Z").unwrap(),
            Some(loc),
            vec![
                "APs".to_string(),
                "f/8".to_string(),
                "lens:50mm".to_string(),
                "camera:fm3a".to_string(),
            ],
            None,
            &profiles,
        );
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
