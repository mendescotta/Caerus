#!/usr/bin/env python3
"""Exercise the bootstrap installer with fixture commands and no network access."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class ReleaseLookupTest(unittest.TestCase):
    def run_installer(self, tags="", lookup_status=0, ref=None):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix="caerus-release-lookup-") as tmp:
            root = Path(tmp)
            tools = root / "tools"
            tools.mkdir()
            fixture = tools / "fixture"
            fixture.write_text(
                "#!/usr/bin/python3\n"
                "import json, os, sys\n"
                "from pathlib import Path\n"
                "name = Path(sys.argv[0]).name\n"
                "args = sys.argv[1:]\n"
                "with open(os.environ['CAERUS_TEST_CALLS'], 'a') as log:\n"
                "    log.write(json.dumps([name, *args]) + '\\n')\n"
                "if name == 'id': print('0')\n"
                "elif name == 'git' and args[0] == 'ls-remote':\n"
                "    print(os.environ['CAERUS_TEST_TAGS'], end='')\n"
                "    sys.exit(int(os.environ['CAERUS_TEST_STATUS']))\n"
                "elif name == 'git' and args[0] == 'clone': Path(args[-1]).mkdir()\n"
                "elif name in ('cargo', 'rustc', 'xbps-query'): pass\n"
                "elif name == 'apk' and args[:2] == ['info', '-e']: pass\n"
                "else: sys.exit(97)\n"
            )
            fixture.chmod(0o755)
            for name in ["git", "cargo", "rustc", "id", "xbps-query", "xbps-install", "apk"]:
                (tools / name).symlink_to(fixture)
            calls = root / "calls.jsonl"
            env = dict(os.environ, PATH=str(tools) + ":/usr/bin:/bin",
                       CAERUS_SRC_DIR=str(root / "checkout"), CAERUS_TEST_CALLS=str(calls),
                       CAERUS_TEST_TAGS=tags, CAERUS_TEST_STATUS=str(lookup_status))
            env.pop("CAERUS_REF", None)
            if ref is not None:
                env["CAERUS_REF"] = ref
            result = subprocess.run(["sh", str(source / "get-caerus.sh")], env=env,
                                    capture_output=True, text=True, start_new_session=True)
            commands = [json.loads(line) for line in calls.read_text().splitlines()]
            return result, commands

    def assert_stopped_before_checkout_or_build(self, result, commands):
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(cmd[:2] == ["git", "clone"] or cmd[0] == "cargo" for cmd in commands))

    def test_lookup_failure_is_not_hidden_by_output_parsing(self):
        result, commands = self.run_installer("abc\trefs/tags/v0.8.0\n", lookup_status=128)
        self.assert_stopped_before_checkout_or_build(result, commands)
        self.assertIn("Could not look up", result.stderr)

    def test_empty_tag_list_does_not_fall_back_to_main(self):
        result, commands = self.run_installer()
        self.assert_stopped_before_checkout_or_build(result, commands)
        self.assertIn("No Caerus release tags", result.stderr)

    def test_latest_release_is_selected(self):
        result, commands = self.run_installer("abc\trefs/tags/v0.8.0\ndef\trefs/tags/v0.7.3\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        clone = next(cmd for cmd in commands if cmd[:2] == ["git", "clone"])
        self.assertEqual(clone[clone.index("--branch") + 1], "v0.8.0")
        self.assertTrue(any(cmd[:2] == ["cargo", "build"] for cmd in commands))

    def test_explicit_refs_skip_release_discovery(self):
        for ref in ["main", "v0.7.3"]:
            with self.subTest(ref=ref):
                result, commands = self.run_installer(lookup_status=128, ref=ref)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertFalse(any(cmd[:2] == ["git", "ls-remote"] for cmd in commands))
                clone = next(cmd for cmd in commands if cmd[:2] == ["git", "clone"])
                self.assertEqual(clone[clone.index("--branch") + 1], ref)


if __name__ == "__main__":
    unittest.main()
