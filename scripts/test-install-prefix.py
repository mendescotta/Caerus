#!/usr/bin/env python3
"""Check prefix validation before any installation or removal is attempted."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class InstallPrefixTest(unittest.TestCase):
    def run_installer(self, prefix, args=()):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix="caerus-prefix-") as tmp:
            tools = Path(tmp)
            script = tools / "install.sh"
            script.write_bytes((source / "install.sh").read_bytes())
            identity = tools / "id"
            identity.write_text("#!/bin/sh\nprintf '1000\\n'\n")
            identity.chmod(0o755)
            sentinel = tools / "sentinel"
            sentinel.write_text("#!/bin/sh\necho 'unexpected file operation' >&2\nexit 96\n")
            sentinel.chmod(0o755)
            for name in ["install", "rm", "mkdir", "find", "ln", "gtk-update-icon-cache", "update-desktop-database"]:
                (tools / name).symlink_to(sentinel)
            env = dict(os.environ, PATH=str(tools) + ":/usr/bin:/bin")
            env.pop("PREFIX", None)
            if prefix is not None:
                env["PREFIX"] = prefix
            return subprocess.run(["sh", str(script), *args], env=env,
                                  capture_output=True, text=True)

    def test_custom_prefix_is_rejected_before_install_or_uninstall(self):
        for prefix in ["/usr/local", "/opt/caerus", "relative", "/usr/../opt"]:
            for args in [(), ("--uninstall",)]:
                with self.subTest(prefix=prefix, args=args):
                    result = self.run_installer(prefix, args)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Unsupported system PREFIX", result.stderr)
                    self.assertNotIn("unexpected file operation", result.stderr)

    def test_default_layout_reaches_normal_root_check(self):
        for prefix in [None, "", "/usr"]:
            with self.subTest(prefix=prefix):
                result = self.run_installer(prefix)
                self.assertIn("System-wide mode needs root", result.stderr)
                self.assertNotIn("Unsupported system PREFIX", result.stderr)

    def test_user_scope_does_not_validate_system_prefix(self):
        result = self.run_installer("/usr/local", ("--user",))
        self.assertIn("Build first", result.stderr)
        self.assertNotIn("Unsupported system PREFIX", result.stderr)
        self.assertNotIn("unexpected file operation", result.stderr)


if __name__ == "__main__":
    unittest.main()
