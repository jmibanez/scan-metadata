Scripts for managing film scan metadata
=======================================

A few scripts I've written to populate EXIF and related metadata on my film scans, including converting from my Day One journal where my bespoke Siri Shortcut writes entries to.


To run:
```
$ scan-metadata --help
Usage: scan-metadata [OPTIONS] <DAYONE_EXPORT_ZIP> <FILELIST>...

Arguments:
  <DAYONE_EXPORT_ZIP>  The path to the exported metadata, as a ZIP file
  <FILELIST>...        Scan files to update

Options:
  -i, --inplace              Modify scans in place
      --dryrun               Dry run; show what would be done to the scans
      --experimental-exif    EXPERIMENTAL: Use pure Rust EXIF implementation
  -p, --profiles <PROFILES>  Use YAML with camera and lens profiles
  -h, --help                 Print help

```
