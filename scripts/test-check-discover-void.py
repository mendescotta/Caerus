#!/usr/bin/env python3
"""Offline regression checks: python3 scripts/test-check-discover-void.py."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("checker", SCRIPTS / "check-discover-void.py")
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


class DiscoverCheckerTests(unittest.TestCase):
    def test_real_catalog_is_fully_parsed(self):
        entries = checker.catalog_entries(SCRIPTS.parent / "caerus/src/ui/catalog.rs")
        self.assertGreater(len(entries), 700)
        self.assertTrue(any("firefox" in app["packages"] for app in entries))

    def test_catalog_parser_allows_parentheses_in_strings(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "catalog.rs"
            path.write_text('pub const CATALOG: &[CatalogApp] = &[\n'
                            '    app!("Development", "GDB (all architectures)", "", "gdb-multiarch"),\n'
                            '    app!(\n        "Web",\n        "Firefox",\n'
                            '        "firefox.desktop",\n        "firefox"\n    ),\n];\n', encoding="utf-8")
            entries = checker.catalog_entries(path)
        self.assertEqual([app["name"] for app in entries], ["GDB (all architectures)", "Firefox"])
        self.assertEqual(entries[0]["packages"], ["gdb-multiarch"])

    def test_search_keeps_hyphenated_names_and_case(self):
        found = checker.parse_search(
            "[-] backintime-qt-1.5.4_2 Simple backup tool - Qt GUI\n"
            "[*] wxMaxima-20.02.4_3 Computer algebra GUI\n")
        self.assertEqual(found["backintime-qt"]["version"], "1.5.4_2")
        self.assertIn("wxMaxima", found)
        with self.assertRaises(ValueError):
            checker.parse_search("unexpected network diagnostic")

    def test_discovery_excludes_covered_aliases_and_support_packages(self):
        rules = json.loads((SCRIPTS / "discover-rules.json").read_text())
        found = checker.parse_search(
            "[-] restic-1_1 Fast backup program\n"
            "[-] cutter-1_1 Reverse engineering platform\n"
            "[-] libbackup-devel-1_1 Backup library\n"
            "[-] ghostty-terminfo-1_1 Terminal emulator - terminfo data\n"
            "[-] oldbackup-1_1 Backup tool - transitional dummy package\n"
            "[-] backup-plugin-1_1 Backup plugin\n"
            "[-] current-alias-1_1 Backup tool\n")
        candidates = checker.discover_candidates(found, [{"packages": ["current-alias"]}], rules)
        self.assertEqual(set(candidates), {"restic", "cutter"})
        self.assertEqual(candidates["cutter"]["suggested_categories"], ["Reverse engineering"])
        self.assertEqual(candidates["restic"]["matched_terms"]["Backup & recovery"], "restic")

    def test_architecture_repository_paths(self):
        for arch, suffix in [("x86_64", "/current"), ("x86_64-musl", "/current/musl"),
                             ("aarch64", "/current/aarch64"), ("aarch64-musl", "/current/aarch64")]:
            self.assertTrue(checker.repository_for_architecture(arch).endswith(suffix))

    def test_malformed_rules_report_status_two(self):
        for field in ("exclude_name", "exclude_description", "categories"):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                rules = json.loads((SCRIPTS / "discover-rules.json").read_text())
                rules[field] = {"Backup": "["} if field == "categories" else "["
                rules_path = Path(directory) / "rules.json"
                rules_path.write_text(json.dumps(rules), encoding="utf-8")
                stdout, stderr = io.StringIO(), io.StringIO()
                with (
                    patch.object(sys, "argv", ["checker", "--discover", "--rules", str(rules_path)]),
                    patch.object(checker, "effective_architecture", return_value="x86_64"),
                    patch.object(checker, "query", return_value="[-] restic-1_1 Backup program\n"),
                    contextlib.redirect_stdout(stdout),
                    contextlib.redirect_stderr(stderr),
                ):
                    status = checker.main()
                self.assertEqual(status, 2)
                self.assertEqual(stdout.getvalue(), "")
                self.assertIn("Void repository check failed:", stderr.getvalue())
                self.assertIn("unterminated character set", stderr.getvalue())

    def test_query_ignores_configured_repos_and_refreshes_in_memory(self):
        with patch.object(checker.subprocess, "run") as run:
            run.return_value.stdout = ""
            checker.query(["-s", "backup"], "https://example.invalid/current")
        arguments = run.call_args.args[0]
        self.assertEqual(arguments[:5], ["xbps-query", "-i", "-M", "--repository",
                                        "https://example.invalid/current"])
        self.assertTrue(run.call_args.kwargs["check"])
        self.assertGreater(run.call_args.kwargs["timeout"], 0)

    def test_desktop_files_handle_symlinks_and_ignore_nonlaunchers(self):
        with patch.object(checker, "query", return_value=(
            "/usr/share/applications/example.desktop -> /opt/example.desktop\n"
            "/usr/share/applications/example.desktop\n/usr/share/doc/example.desktop\n"
            "/usr/bin/example\n")):
            self.assertEqual(checker.desktop_files("example", "repository"),
                             ("example", ["example.desktop"]))


if __name__ == "__main__":
    unittest.main()
