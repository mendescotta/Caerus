#!/usr/bin/env python3
"""Check package names against Void's official binary repository, without installing.

Usage: python3 scripts/check-discover-void.py pika-backup gnucash openscad
       python3 scripts/check-discover-void.py --desktop-files pika-backup gnucash
       python3 scripts/check-discover-void.py --discover --nonfree --output candidates.json
       python3 scripts/check-discover-void.py --catalog --nonfree
       python3 scripts/check-discover-void.py --output /tmp/void-apps.json PACKAGE...

Requires xbps-query and network access; xbps-uhelper determines the architecture
unless XBPS_ARCH is set. Repository metadata is fetched in memory; configured
third-party repositories are ignored. Results describe the host XBPS architecture
only, not other architectures or installability. --desktop-files also queries
each matching binary package's file list (additional downloads). Discovery uses
scripts/discover-rules.json to suggest uncatalogued names for human review; a
keyword match is not evidence of a GUI, category suitability, or installability.
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

REPOSITORY_BASE = "https://repo-default.voidlinux.org/current"
ROOT = Path(__file__).resolve().parents[1]


def catalog_entries(path):
    """Read the literal app! entries, refusing silent partial catalog audits."""
    source = path.read_text(encoding="utf-8").split("pub const CATALOG:", 1)[1]
    source = source.split("\n];", 1)[0]
    entries = []
    # String literals may contain parentheses, so skip over them when finding ")".
    for match in re.finditer(r'app!\(((?:"(?:[^"\\]|\\.)*"|[^")])*)\)', source):
        body = match.group(1)
        fields = re.findall(r'"(?:[^"\\]|\\.)*"', body)
        if len(fields) < 4 or re.sub(r'"(?:[^"\\]|\\.)*"|[\s,]', "", body):
            raise ValueError("unsupported app! entry; update catalog parser")
        category, name, desktop, *packages = map(json.loads, fields)
        entries.append(dict(category=category, name=name, desktop=desktop, packages=packages))
    if not entries or len(entries) != source.count("app!("):
        raise ValueError("could not read every catalog entry")
    return entries


def parse_search(output):
    found = {}
    for line in output.splitlines():
        fields = line.split(maxsplit=2)
        if len(fields) != 3 or fields[0] not in ("[-]", "[*]"):
            raise ValueError(f"unexpected XBPS search output: {line!r}")
        name, separator, version = fields[1].rpartition("-")
        if not separator or not version:
            raise ValueError(f"invalid package version: {fields[1]!r}")
        found[name] = {"version": version, "description": fields[2]}
    return found


def discover_candidates(packages, entries, rules):
    """Suggestions are review inputs, never automatic catalog additions."""
    covered = {name for app in entries for name in app["packages"]}
    exclude = re.compile(rules["exclude_name"], re.IGNORECASE)
    exclude_description = re.compile(rules["exclude_description"], re.IGNORECASE)
    patterns = {category: re.compile(pattern, re.IGNORECASE)
                for category, pattern in rules["categories"].items()}
    candidates = {}
    for name, metadata in sorted(packages.items()):
        if name in covered or exclude.search(name) or exclude_description.search(metadata["description"]):
            continue
        evidence = {category: match.group(0) for category, pattern in patterns.items()
                    if (match := pattern.search(name + " " + metadata["description"]))}
        if evidence:
            candidates[name] = {**metadata, "suggested_categories": list(evidence),
                                "matched_terms": evidence}
    return candidates


def effective_architecture():
    architecture = os.environ.get("XBPS_ARCH")
    if architecture:
        return architecture
    return subprocess.run(
        ["xbps-uhelper", "arch"],
        capture_output=True, text=True, check=True, timeout=30,
        env={**os.environ, "LC_ALL": "C"},
    ).stdout.strip()


def repository_for_architecture(architecture):
    suffix = ""
    if architecture.startswith("aarch64"):
        suffix = "/aarch64"
    elif architecture.endswith("-musl"):
        suffix = "/musl"
    return REPOSITORY_BASE + suffix


def package_name(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9+_.-]*", value):
        raise argparse.ArgumentTypeError(f"invalid package name: {value!r}")
    return value


def query(arguments, repository):
    return subprocess.run(
        ["xbps-query", "-i", "-M", "--repository", repository, *arguments],
        capture_output=True, text=True, check=True, timeout=180,
        env={**os.environ, "LC_ALL": "C"},
    ).stdout


def desktop_files(name, repository):
    paths = (line.split(" -> ", 1)[0]
             for line in query(["-f", name], repository).splitlines())
    return name, sorted({
        path.removeprefix("/usr/share/applications/") for path in paths
        if path.startswith("/usr/share/applications/") and path.endswith(".desktop")
    })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", nargs="*", type=package_name)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--discover", action="store_true",
                      help="suggest uncatalogued packages using reviewed search rules")
    mode.add_argument("--catalog", action="store_true", help="check all catalog package aliases")
    parser.add_argument("--rules", type=Path, default=ROOT / "scripts/discover-rules.json")
    parser.add_argument("--catalog-path", type=Path, default=ROOT / "caerus/src/ui/catalog.rs")
    parser.add_argument("--nonfree", action="store_true",
                        help="also check the official nonfree repository")
    parser.add_argument("--output", type=Path, help="save JSON instead of printing it")
    parser.add_argument("--desktop-files", action="store_true",
                        help="also fetch desktop filenames from matching binary packages")
    args = parser.parse_args()
    if not (args.packages or args.discover or args.catalog):
        parser.error("provide package names, --catalog, or --discover")
    if args.packages and (args.discover or args.catalog):
        parser.error("package names cannot be combined with --catalog or --discover")
    if args.discover and args.desktop_files:
        parser.error("review discovery suggestions, then query selected names with --desktop-files")
    requested = sorted(set(args.packages))
    # Escape literal names for XBPS's extended regular expression search. Exact
    # names are selected below, excluding similarly named libraries/subpackages.
    try:
        entries = catalog_entries(args.catalog_path) if args.catalog or args.discover else []
        if args.catalog:
            requested = sorted({name for app in entries for name in app["packages"]})
        pattern = ".*" if args.discover else "|".join(re.escape(name) for name in requested)
        architecture = effective_architecture()
        if not architecture:
            raise ValueError("could not determine XBPS architecture")
        repository = repository_for_architecture(architecture)
        repositories = [repository]
        if args.nonfree:
            repositories.append(repository + "/nonfree")
        found = {}
        for source in repositories:
            for name, metadata in parse_search(query(["--regex", "-s", pattern], source)).items():
                if (args.discover or name in requested) and name not in found:
                    found[name] = {**metadata, "repository": source}
        scanned = len(found)
        if args.discover:
            rules = json.loads(args.rules.read_text(encoding="utf-8"))
            found = discover_candidates(found, entries, rules)
        missing = sorted(set(requested) - found.keys())
        if args.desktop_files:
            # Bound parallel downloads; an empty list is valid for metapackages.
            with ThreadPoolExecutor(max_workers=4) as executor:
                for name, paths in executor.map(
                    lambda name: desktop_files(name, found[name]["repository"]), found
                ):
                    found[name]["desktop_files"] = paths
        report = {
            "repository": repository,
            "repositories": repositories,
            "architecture": architecture,
            "mode": "discover" if args.discover else "catalog" if args.catalog else "verify",
            "scanned_packages": scanned,
            "checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "architecture_override": os.environ.get("XBPS_ARCH"),
            "machine": os.uname().machine,
            "packages": found,
            "missing": missing,
        }
        if args.catalog:
            report["unavailable_apps"] = [app["name"] for app in entries
                                          if not any(p in found for p in app["packages"])]
            report["available_apps"] = len(entries) - len(report["unavailable_apps"])
            report["catalog_apps"] = len(entries)
            report["available_by_category"] = {
                category: sum(app["category"] == category and
                              any(p in found for p in app["packages"]) for app in entries)
                for category in dict.fromkeys(app["category"] for app in entries)
            }
        output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.write_text(output, encoding="utf-8")
        else:
            sys.stdout.write(output)
        return int(bool(missing))
    except (OSError, ValueError, KeyError, IndexError, re.error, subprocess.SubprocessError) as error:
        print(f"Void repository check failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.strip(), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
