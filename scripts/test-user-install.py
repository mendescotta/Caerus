#!/usr/bin/env python3
"""Check the user launcher with GIO, using only temporary files and a fake app."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest


class UserInstallTest(unittest.TestCase):
    def test_launcher_preserves_executable_path_and_exec_field_codes(self):
        if os.getuid() == 0:
            self.skipTest("the user installer intentionally refuses root")
        if shutil.which("gio") is None:
            self.skipTest("GIO is required to exercise desktop Exec parsing")
        source = Path(__file__).resolve().parents[1]
        for checkout_name in ["checkout with spaces", 'checkout & | " $ ` \\', "checkout % %f"]:
            with self.subTest(checkout=checkout_name), tempfile.TemporaryDirectory(prefix="caerus-install-") as tmp:
                root = Path(tmp)
                checkout = root / checkout_name
                binary = checkout / "target/release/caerus"
                binary.parent.mkdir(parents=True)
                binary.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json, os, sys\n"
                    "from pathlib import Path\n"
                    "Path(os.environ['CAERUS_TEST_RESULT']).write_text(json.dumps(sys.argv))\n"
                )
                binary.chmod(0o755)
                (checkout / "caerus").mkdir()
                (checkout / "caerus/data").symlink_to(source / "caerus/data", target_is_directory=True)
                shutil.copyfile(source / "install.sh", checkout / "install.sh")
                result = root / "argv.json"
                env = dict(os.environ, XDG_DATA_HOME=str(root / "data"), CAERUS_TEST_RESULT=str(result))
                subprocess.run(["sh", str(checkout / "install.sh"), "--user"], env=env, check=True, capture_output=True)
                desktop = root / "data/applications/org.voidlinux.caerus.desktop"
                launch = subprocess.run(["gio", "launch", str(desktop)], env=env, capture_output=True, text=True)
                self.assertEqual(launch.returncode, 0, desktop.read_text() + launch.stderr)
                deadline = time.monotonic() + 5
                while not result.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(result.exists(), "the registered launcher must run the fake app")
                self.assertEqual(json.loads(result.read_text()), [str(binary)])


if __name__ == "__main__":
    unittest.main()
