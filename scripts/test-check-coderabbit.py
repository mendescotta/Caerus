#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# dependencies = ["PyYAML>=6,<7", "jsonschema>=4.18,<5"]
# ///
"""Offline regression checks: uv run scripts/test-check-coderabbit.py."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location(
    "checker", Path(__file__).with_name("check-coderabbit.py")
)
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)

SCHEMA = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {"reviews": {"type": "object"}, "chat": {"type": "object"}},
    "additionalProperties": False,
}


class CodeRabbitCheckerTests(unittest.TestCase):
    def run_checker(self, schema, config="reviews: {}\nchat: {}\n"):
        with tempfile.TemporaryDirectory() as directory:
            config_path = Path(directory) / "config.yaml"
            config_path.write_text(config, encoding="utf-8")
            schema_path = Path(directory) / "schema.json"
            schema_path.write_text(json.dumps(schema), encoding="utf-8")
            stdout, stderr = io.StringIO(), io.StringIO()
            with patch.object(sys, "argv", ["checker", "--config", str(config_path),
                                            "--schema", str(schema_path)]):
                with patch.object(checker.urllib.request, "urlopen",
                                  side_effect=AssertionError("offline validation fetched a schema")):
                    with contextlib.redirect_stdout(stdout):
                        with contextlib.redirect_stderr(stderr):
                            status = checker.main()
            return status, stdout.getvalue(), stderr.getvalue()

    def test_rejects_wrong_schema_json_types(self):
        for schema in (None, True, False, [], ["$schema"], 1, "schema"):
            with self.subTest(schema=schema):
                status, stdout, stderr = self.run_checker(schema)
                self.assertEqual(status, 2)
                self.assertEqual(stdout, "")
                self.assertIn("Cannot validate CodeRabbit configuration:", stderr)
                self.assertIn(checker.SCHEMA_URL, stderr)

    def test_rejects_schemas_without_coderabbit_structure(self):
        for schema in ({}, {"type": "array"}, {"type": "object"},
                       {"type": "object", "properties": []},
                       {"type": "object", "properties": {"unrelated": {}}},
                       {**SCHEMA, "properties": {"reviews": {"type": "object"}}},
                       {**SCHEMA, "properties": {"reviews": {}, "chat": {"type": "string"}}}):
            with self.subTest(schema=schema):
                status, stdout, stderr = self.run_checker(schema)
                self.assertEqual(status, 2)
                self.assertEqual(stdout, "")
                self.assertIn(checker.SCHEMA_URL, stderr)

    def test_rejects_invalid_meta_schema_types(self):
        for value in (None, [], {}, 1, True):
            with self.subTest(value=value):
                status, stdout, stderr = self.run_checker({**SCHEMA, "$schema": value})
                self.assertEqual(status, 2)
                self.assertEqual(stdout, "")
                self.assertIn("$schema", stderr)

    def test_rejects_malformed_json_schema(self):
        status, stdout, stderr = self.run_checker({**SCHEMA, "additionalProperties": "invalid"})
        self.assertEqual(status, 2)
        self.assertEqual(stdout, "")
        self.assertIn("Cannot validate CodeRabbit configuration:", stderr)

    def test_valid_configuration_succeeds_offline(self):
        status, stdout, stderr = self.run_checker(SCHEMA)
        self.assertEqual(status, 0)
        self.assertIn("Valid CodeRabbit configuration:", stdout)
        self.assertEqual(stderr, "")

    def test_configuration_errors_keep_status_one(self):
        for config, message in (("unexpected: true\n", "Additional properties"),
                                ("reviews: false\n", "not of type 'object'"),
                                ("reviews: {}\nreviews: {}\n", "duplicate mapping key")):
            with self.subTest(config=config):
                status, stdout, stderr = self.run_checker(SCHEMA, config)
                self.assertEqual(status, 1)
                self.assertEqual(stdout, "")
                self.assertIn(message, stderr)


if __name__ == "__main__":
    unittest.main()
