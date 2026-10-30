#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# dependencies = ["PyYAML>=6,<7", "jsonschema>=4.18,<5"]
# ///
"""Validate .coderabbit.yaml with CodeRabbit's official JSON schema.

Run: uv run scripts/check-coderabbit.py
Offline: uv run scripts/check-coderabbit.py --schema /path/to/schema.v2.json
An existing Python environment with PyYAML/jsonschema can run this script directly.
Exit 0 means valid, 1 means invalid configuration, 2 means a validation setup error.
"""

import argparse
import json
from pathlib import Path
import sys
import urllib.request

SCHEMA_URL = "https://coderabbit.ai/integrations/schema.v2.json"
DEFAULT_CONFIG = Path(__file__).resolve().parents[1] / ".coderabbit.yaml"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--schema", type=Path, help="use a saved schema without fetching it")
    args = parser.parse_args()
    try:
        import jsonschema
        import yaml
    except ImportError:
        print("Run with dependencies: uv run scripts/check-coderabbit.py", file=sys.stderr)
        return 2

    class UniqueKeyLoader(yaml.SafeLoader):
        def construct_mapping(self, node, deep=False):
            # YAML otherwise silently overwrites duplicate review/tool settings.
            mapping = {}
            for key, value in self.construct_pairs(node, deep=deep):
                try:
                    duplicate = key in mapping
                except TypeError as error:
                    raise yaml.constructor.ConstructorError(
                        None, None, "mapping keys must be hashable", node.start_mark
                    ) from error
                if duplicate:
                    raise yaml.constructor.ConstructorError(
                        None, None, f"duplicate mapping key: {key}", node.start_mark
                    )
                mapping[key] = value
            return mapping

    try:
        config = yaml.load(args.config.read_text(encoding="utf-8"), Loader=UniqueKeyLoader)
    except yaml.YAMLError as error:
        print(f"Invalid YAML: {error}", file=sys.stderr)
        return 1
    except (OSError, UnicodeError) as error:
        print(f"Cannot read configuration: {error}", file=sys.stderr)
        return 2

    try:
        if args.schema:
            schema = json.loads(args.schema.read_text(encoding="utf-8"))
        else:
            with urllib.request.urlopen(SCHEMA_URL, timeout=30) as response:
                schema = json.load(response)
        if not isinstance(schema, dict) or schema.get("type") != "object":
            raise ValueError(f"schema must be a JSON object with type 'object'; use {SCHEMA_URL}")
        properties = schema.get("properties")
        if not isinstance(properties, dict) or any(
            not isinstance(properties.get(name), dict) or properties[name].get("type") != "object"
            for name in ("reviews", "chat")
        ):
            raise ValueError(
                f"schema must define CodeRabbit 'reviews' and 'chat' object properties; use {SCHEMA_URL}"
            )
        if "$schema" in schema and not isinstance(schema["$schema"], str):
            raise ValueError(f"schema '$schema' must be a JSON string; use {SCHEMA_URL}")
        validator_type = jsonschema.validators.validator_for(schema)
        validator_type.check_schema(schema)
        errors = sorted(validator_type(schema).iter_errors(config),
                        key=lambda error: tuple(map(str, error.absolute_path)))
    except (OSError, ValueError, jsonschema.exceptions.SchemaError) as error:
        print(f"Cannot validate CodeRabbit configuration: {error}", file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            path = ".".join(map(str, error.absolute_path)) or "<root>"
            print(f"{path}: {error.message}", file=sys.stderr)
        return 1
    print(f"Valid CodeRabbit configuration: {args.config}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
