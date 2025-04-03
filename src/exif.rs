use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct ExifTag {
    name: String,
    value: TagValue,
}

#[derive(Debug)]
pub enum TagValue {
    String(String),
    List(Vec<String>),
}

pub trait ExifTagTrait {
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

pub struct ExifProcessorOptions {
    pub inplace: bool,
    pub dryrun: bool,
}

pub trait ExifProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &Vec<ExifTag>,
        options: &ExifProcessorOptions,
    ) -> bool;
}

struct ExifToolProcessor {}

impl ExifToolProcessor {
    fn to_exiftool_cmd_line(
        &self,
        filepath: &Path,
        exif_tags: &Vec<ExifTag>,
        options: &ExifProcessorOptions,
    ) -> Vec<String> {
        let mut args = Vec::new();

        args.push("-q".to_string());

        if options.inplace {
            args.push("-overwrite_original_in_place".to_string());
        }

        for tag in exif_tags.iter() {
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

impl ExifProcessor for ExifToolProcessor {
    fn write_out_exif(
        &self,
        filepath: &Path,
        exif_tags: &Vec<ExifTag>,
        options: &ExifProcessorOptions,
    ) -> bool {
        let args = self.to_exiftool_cmd_line(filepath, exif_tags, options);
        if !options.dryrun {
            println!("Updating tags for {}", filepath.display());
            let mut proc = Command::new("exiftool").args(&args).spawn().unwrap();
            let result = proc.wait().unwrap();
            result.success()
        } else {
            let cmd = args.join(" \\\n\t\t");
            println!("Would have updated {}", filepath.display());
            println!("\texiftool {}", cmd);
            println!();
            true
        }
    }
}

pub fn get_default_processor() -> Box<dyn ExifProcessor> {
    Box::new(ExifToolProcessor {})
}
