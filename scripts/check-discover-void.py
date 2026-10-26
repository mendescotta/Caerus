#!/usr/bin/env python3
"""Check package names against Void's official binary repository, without installing.

Usage: python3 scripts/check-discover-void.py pika-backup gnucash openscad
       python3 scripts/check-discover-void.py --desktop-files pika-backup gnucash
       python3 scripts/check-discover-void.py --output /tmp/void-apps.json PACKAGE...

Requires xbps-query and network access. Repository metadata is fetched in memory;
configured third-party repositories are ignored. Results describe the host XBPS
architecture only, not other architectures or installability. --desktop-files
also queries each matching binary package's file list (additional downloads).
Exit 0 means every requested name was found; 1 means missing names; 2 is an error.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys

REPOSITORY = "https://repo-default.voidlinux.org/current"


def package_name(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9+_.-]*", value):
        raise argparse.ArgumentTypeError(f"invalid package name: {value!r}")
    return value


def query(arguments):
    return subprocess.run(
        ["xbps-query", "-i", "-M", "--repository", REPOSITORY, *arguments],
        capture_output=True, text=True, check=True, timeout=180,
        env={**os.environ, "LC_ALL": "C"},
    ).stdout


def desktop_files(name):
    paths = (line.split(" -> ", 1)[0] for line in query(["-f", name]).splitlines())
    return name, sorted({
        path.removeprefix("/usr/share/applications/") for path in paths
        if path.startswith("/usr/share/applications/") and path.endswith(".desktop")
    })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", nargs="+", type=package_name)
    parser.add_argument("--output", type=Path, help="save JSON instead of printing it")
    parser.add_argument("--desktop-files", action="store_true",
                        help="also fetch desktop filenames from matching binary packages")
    args = parser.parse_args()
    requested = sorted(set(args.packages))
    # Escape literal names for XBPS's extended regular expression search. Exact
    # names are selected below, excluding similarly named libraries/subpackages.
    pattern = "|".join(re.escape(name) for name in requested)
    try:
        result = query(["--regex", "-s", pattern])
        found = {}
        for line in result.splitlines():
            fields = line.split(maxsplit=2)
            if len(fields) != 3 or fields[0] not in ("[-]", "[*]"):
                raise ValueError(f"unexpected XBPS search output: {line!r}")
            name, separator, version = fields[1].rpartition("-")
            if not separator or not version:
                raise ValueError(f"invalid package version: {fields[1]!r}")
            if name in requested:
                found[name] = {"version": version, "description": fields[2]}
        missing = sorted(set(requested) - found.keys())
        if args.desktop_files:
            # Bound parallel downloads; an empty list is valid for metapackages.
            with ThreadPoolExecutor(max_workers=4) as executor:
                for name, paths in executor.map(desktop_files, found):
                    found[name]["desktop_files"] = paths
        report = {
            "repository": REPOSITORY,
            "checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "architecture_override": os.environ.get("XBPS_ARCH"),
            "machine": os.uname().machine,
            "packages": found,
            "missing": missing,
        }
        output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.write_text(output, encoding="utf-8")
        else:
            sys.stdout.write(output)
        return int(bool(missing))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Void repository check failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.strip(), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
