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
    def __init__(self, frame_count, entry_date, location, tags):
        self.frame_count = frame_count
        self.entry_date = entry_date
        self.location = location
        self.tags = tags

    def write_to_exif(self, filepath: Path, overwrite_original: bool = False):
        tag_args = {
            "Keywords": self.tags,
            "DateTimeOriginal": self.munge_date_with_framecount(),
        }
        self.populate_location_args(tag_args)
        args = ["exiftool"]

        if overwrite_original:
            args.append("-overwrite_original_in_place")

        for k, v in tag_args.items():
            if isinstance(v, list):
                for i in v:
                    args.append(f"-{k}+={i}")
            else:
                args.append(f"-{k}={v}")

        args.append(str(filepath))
        print(f"Updating tags for {str(filepath)}")
        subprocess.run(args)

    def munge_date_with_framecount(self):
        date_string = self.entry_date
        d = datetime.fromisoformat(date_string)
        munged_datetime = d.replace(second=int(self.frame_count))

        tzname = self.location["timeZoneName"].replace("\\/", "/")
        tz = pytz.timezone(tzname)

        return munged_datetime.astimezone(tz)

    def populate_location_args(self, tag_args):
        if not self.location:
            return

        if "region" in self.location:
            loc_region = self.location["region"]
            lat = loc_region["center"]["latitude"]
            lon = loc_region["center"]["longitude"]
            radius = loc_region["radius"]

            tag_args["GPSLatitude"] = tag_args["GPSLatitudeRef"] = lat
            tag_args["GPSLongitude"] = tag_args["GPSLongitudeRef"] = lon
            tag_args["GPSHPositioningError"] = radius

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

def match_files_to_entries(scan_dir: str, prefix: str, metadata_entries: List[MetadataEntry], overwrite: bool):
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
        metadata_entry = metadata_map[frame_count]
        metadata_entry.write_to_exif(s, overwrite)

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
    parser.add_argument("prefix")
    parser.add_argument("dayone_export_zipfile")

    ns = parser.parse_args()

    scan_dir = ns.scandir
    prefix = ns.prefix
    dayone_export_zipfile = ns.dayone_export_zipfile
    overwrite = ns.inplace

    json_dict = dayone_export_zip_to_json(dayone_export_zipfile)
    metadata_entries = read_entries(json_dict)
    match_files_to_entries(scan_dir, prefix, metadata_entries, overwrite)


if __name__ == '__main__':
    dayone_export_to_exif()
