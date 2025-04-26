Scripts for managing film scan metadata
=======================================

A few scripts I've written to populate EXIF and related metadata on my film scans, including converting from my Day One journal where my bespoke Siri Shortcut writes entries to.


Utilities included:

```
$ update_scan_exif --help
Usage: update_scan_exif [OPTIONS] <DAYONE_EXPORT_ZIP> <FILELIST>...

Arguments:
  <DAYONE_EXPORT_ZIP>  The path to the exported metadata, as a ZIP file
  <FILELIST>...        Scan files to update

Options:
  -q, --quiet                Quiet; minimize output to errors
      --debug                Turn on debug logging
  -i, --inplace              Modify scans in place
      --dryrun               Dry run; show what would be done to the scans
      --legacy-exif          Legacy: Fork exiftool instead of using internal EXIF processor
  -p, --profiles <PROFILES>  Use YAML with camera and lens profiles
  -h, --help                 Print help

```
