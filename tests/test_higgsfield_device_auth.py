from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


MODULE_PATH = Path(__file__).resolve().parents[1] / "scripts" / "higgsfield_device_auth.py"
SPEC = importlib.util.spec_from_file_location("higgsfield_device_auth", MODULE_PATH)
assert SPEC is not None
assert SPEC.loader is not None
higgsfield_device_auth = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(higgsfield_device_auth)


class HiggsfieldDeviceAuthSmokeTest(unittest.TestCase):
    def test_required_smoke_tools_are_accepted(self) -> None:
        tools = [
            {"name": name}
            for name in sorted(higgsfield_device_auth.REQUIRED_SMOKE_TOOLS)
        ]

        higgsfield_device_auth.assert_required_tools_present(tools)

    def test_missing_required_smoke_tool_fails(self) -> None:
        tools = [
            {"name": name}
            for name in sorted(
                higgsfield_device_auth.REQUIRED_SMOKE_TOOLS - {"job_status"}
            )
        ]

        with self.assertRaisesRegex(RuntimeError, "job_status"):
            higgsfield_device_auth.assert_required_tools_present(tools)


if __name__ == "__main__":
    unittest.main()
