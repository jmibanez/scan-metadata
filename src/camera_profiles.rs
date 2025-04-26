use log::debug;
use serde::Deserialize;

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;

use crate::models::MetadataReadError;

const PROJECT_QUALIFER: &str = "com";
const PROJECT_ORGANIZATION: &str = "jmibanez";
const PROJECT_APPNAME: &str = "scan-metadata";

#[derive(Clone, Deserialize, Debug)]
pub struct CameraLensProfile {
    pub name: String,
    pub tag: String,
    pub exif_tags: Option<HashMap<String, String>>,
    pub lens_profiles: Vec<LensProfile>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct LensProfile {
    pub name: String,
    pub tag: String,
    pub min_focal_length_mm: u16,
    pub max_focal_length_mm: u16,
    // min_aperture: f32,
    pub max_aperture_at_short: f32,
    pub max_aperture_at_long: f32,

    pub exif_tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Default)]
pub struct CameraProfileMapEntry {
    pub name: String,
    pub lens_profiles: HashMap<String, LensProfile>,
    pub exif_tags: Option<HashMap<String, String>>,
}

#[derive(Default)]
pub struct CameraProfileMap {
    cameras: HashMap<String, CameraProfileMapEntry>,
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

pub fn read_camera_profiles_fallback_to_prefs(
    camera_profiles_file: Option<PathBuf>,
) -> Result<Option<Vec<CameraLensProfile>>, MetadataReadError> {
    match camera_profiles_file {
        Some(file) => read_camera_profiles_yaml(file.as_path()),
        None => {
            if let Some(project) =
                ProjectDirs::from(PROJECT_QUALIFER, PROJECT_ORGANIZATION, PROJECT_APPNAME)
            {
                let mut user_prefs_profiles = project.config_dir().to_path_buf();
                user_prefs_profiles.push("camera_profiles.yaml");
                if user_prefs_profiles.exists() {
                    read_camera_profiles_yaml(user_prefs_profiles.as_path())
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
    }
}

fn read_camera_profiles_yaml(
    camera_profiles_file: &Path,
) -> Result<Option<Vec<CameraLensProfile>>, MetadataReadError> {
    let f = File::open(camera_profiles_file)?;
    let yaml = serde_yaml::from_reader(f)?;
    Ok(Some(yaml))
}

impl CameraProfileMap {
    pub fn new(maybe_camera_profiles: Option<Vec<CameraLensProfile>>) -> Self {
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

    pub fn get_profile(
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
