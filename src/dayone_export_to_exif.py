#!/usr/bin/env python3

import pytz

from argparse import ArgumentParser
from datetime import datetime
from json import loads as json_loads
from pathlib import Path
import re
import subprocess
from zipfile import ZipFile

from typing import List

class MetadataEntry(object):
    def __init__(self, frame_count, entry_date, location, entry_tags):
        self.frame_count = frame_count
        self.entry_date = entry_date
        self.location = location
        self.entry_tags = entry_tags

        self.exif_tags = dict()
        self.populate_tags()

    def populate_tags(self):
        self.add_exif_tag("Keywords", self.entry_tags)
        self.add_exif_tag("DateTimeOriginal", self.munge_date_with_framecount())
        self.populate_location_tags()
        self.populate_from_entry_tags()

    def get_exiftool_command_line(self, filepath: Path, overwrite_original: bool = False):
        args = ["exiftool"]

        if overwrite_original:
            args.append("-overwrite_original_in_place")

        for k, v in self.exif_tags.items():
            if isinstance(v, list):
                for i in v:
                    args.append(f"-{k}+={i}")
            else:
                args.append(f"-{k}={v}")

        args.append(str(filepath))
        return args

    def write_to_exif(self, filepath: Path, overwrite_original: bool = False):
        args = self.get_exiftool_command_line(filepath, overwrite_original)
        print(f"Updating tags for {str(filepath)}")
        subprocess.run(args)

    def munge_date_with_framecount(self):
        date_string = self.entry_date
        d = datetime.fromisoformat(date_string)
        munged_datetime = d.replace(second=int(self.frame_count))

        if self.location and "timeZoneName" in self.location:
            tzname = self.location["timeZoneName"].replace("\\/", "/")
            tz = pytz.timezone(tzname)
        else:
            print(f"Warning: Frame {self.frame_count} does not have TZ info, using current local TZ")
            tz = datetime.now().astimezone().tzinfo

        return munged_datetime.astimezone(tz)

    def populate_location_tags(self):
        if not self.location:
            return

        if "region" in self.location:
            loc_region = self.location["region"]
            lat = loc_region["center"]["latitude"]
            lon = loc_region["center"]["longitude"]
            radius = loc_region["radius"]

            self.add_exif_tag("GPSLatitude", lat)
            self.add_exif_tag("GPSLatitudeRef", lat)
            self.add_exif_tag("GPSLongitude", lon)
            self.add_exif_tag("GPSLongitudeRef", lon)
            self.add_exif_tag("GPSHPositioningError", radius)


    def populate_from_entry_tags(self):
        shutter_tag = next(filter(_is_shutter_tag, self.entry_tags), None)
        if shutter_tag:
            if shutter_tag != 'APs':
                shutter_speed = shutter_tag[:-1]
                self.add_exif_tag("ShutterSpeedValue", shutter_speed)
            self.entry_tags.remove(shutter_tag)

        aperture_tag = next(filter(_is_aperture_tag, self.entry_tags), None)
        if aperture_tag:
            aperture = aperture_tag[2:]
            self.add_exif_tag("ApertureValue", aperture)
            self.entry_tags.remove(aperture_tag)

        lens_tag = next(filter(_is_lens_tag, self.entry_tags), None)
        if lens_tag:
            focal_length = parse_lens_tag(lens_tag)
            if focal_length:
                self.add_exif_tag("FocalLength", focal_length)

        if "unindexed" in self.entry_tags:
            self.entry_tags.remove("unindexed")
        if "scanned"  in self.entry_tags:
            self.entry_tags.remove("scanned")

    def add_exif_tag(self, name, value):
        self.exif_tags[name] = value


def _is_aperture_tag(tag):
    return tag[0:2] == "f/"

_SHUTTER_TAG_MATCHER = re.compile("(1/)?\\d+s")
def _is_shutter_tag(tag):
    return _SHUTTER_TAG_MATCHER.match(tag)

def _is_lens_tag(tag):
    return tag.startswith("lens:")

_LENS_FOCAL_LENGTH_MATCHER = re.compile("(\\d+mm)")
def parse_lens_tag(tag):
    m = _LENS_FOCAL_LENGTH_MATCHER.match(tag)
    if not m:
        return None
    return m.group(1)

def parse_frame_count(text):
    lines = text.split('\n')
    header = lines[0]
    frame_count = header.split(' ')[1]
    return frame_count

def extract_metadata_from_entry(e):
    location = e.get('location')
    tags = e['tags']
    entry_date = e['creationDate']
    text = e['text']
    frame_count = parse_frame_count(text)

    return MetadataEntry(frame_count, entry_date, location, tags)

def read_entries(json_dict):
    json_entries = json_dict['entries']
    metadata_entries = [ extract_metadata_from_entry(e) for e in json_entries ]
    return metadata_entries

def match_files_to_entries(
    scan_dir: str,
    prefix: str,
    metadata_entries: List[MetadataEntry],
    overwrite: bool,
    dryrun: bool,
):
    # Expect (prefix)_(\d\d\d\d)
    p = Path(scan_dir)
    scans_to_apply = sorted(p.glob(f"{prefix}_*.tif"))
    metadata_map = {
        int(e.frame_count): e
        for e in metadata_entries
    }
    entry_matcher = re.compile(f"({prefix})_(0+)(\\d+)")
    for s in scans_to_apply:
        filename = s.stem
        m = entry_matcher.match(filename)
        if not m:
            continue
        frame_count = int(m.group(3))
        if frame_count not in metadata_map:
            continue
        metadata_entry = metadata_map[frame_count]
        if not dryrun:
            metadata_entry.write_to_exif(s, overwrite)
        else:
            args = metadata_entry.get_exiftool_command_line(s, overwrite)
            cmd = " ".join(args)
            print(f"Would have updated {str(s)}...")
            print(f"\t{cmd}")
            print()

def dayone_export_zip_to_json(f):
    with ZipFile(f) as z:
        journal_bytes = z.read("Journal.json")
        json_dict = json_loads(journal_bytes)
        return json_dict

def dayone_export_to_exif():
    parser = ArgumentParser(
        description="Populate metadata for TIFF scans based on Day One journal entries",
    )
    parser.add_argument("--scandir", "-s", default=".", required=False)
    parser.add_argument("--inplace", "-i", action='store_true')
    parser.add_argument("--dryrun",  action='store_true')
    parser.add_argument("prefix")
    parser.add_argument("dayone_export_zipfile")

    ns = parser.parse_args()

    scan_dir = ns.scandir
    prefix = ns.prefix
    dayone_export_zipfile = ns.dayone_export_zipfile
    overwrite = ns.inplace
    dryrun = ns.dryrun

    json_dict = dayone_export_zip_to_json(dayone_export_zipfile)
    metadata_entries = read_entries(json_dict)
    match_files_to_entries(scan_dir, prefix, metadata_entries,
                           overwrite, dryrun)


if __name__ == '__main__':
    dayone_export_to_exif()
