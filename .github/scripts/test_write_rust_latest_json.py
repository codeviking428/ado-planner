#!/usr/bin/env python3
"""Unit tests for write-rust-latest-json.py."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

_SCRIPT = Path(__file__).resolve().parent / "write-rust-latest-json.py"
_SPEC = importlib.util.spec_from_file_location("write_rust_latest_json", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)
collect = _MOD.collect
platform_for = _MOD.platform_for


class PlatformForTests(unittest.TestCase):
    def test_nsis_installer(self) -> None:
        self.assertEqual(
            platform_for(Path("ado-planner-v0.3.0-x86_64-setup.exe")),
            (["windows-x86_64"], "nsis"),
        )

    def test_appimage(self) -> None:
        self.assertEqual(
            platform_for(Path("ado-planner-v0.3.0-x86_64.AppImage")),
            (["linux-x86_64"], "appimage"),
        )

    def test_macos_artifacts_are_not_updater_artifacts(self) -> None:
        self.assertIsNone(platform_for(Path("ADO Planner.app.tar.gz")))
        self.assertIsNone(platform_for(Path("ADO Planner.dmg")))

    def test_signatures_are_not_artifacts(self) -> None:
        self.assertIsNone(platform_for(Path("ado-planner-v0.3.0-x86_64.AppImage.sig")))


class CollectTests(unittest.TestCase):
    def test_skips_unsigned_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp)
            (dist / "ado-planner-v0.3.0-x86_64.AppImage").write_bytes(b"bin")

            platforms = collect(dist, "org/repo", "v0.3.0")

        self.assertEqual(platforms, {})

    def test_full_matrix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp)
            files = [
                "ado-planner-v1.0.0-x86_64-setup.exe",
                "ado-planner-v1.0.0-x86_64.AppImage",
                "ADO Planner.app.tar.gz",
                "ADO Planner.dmg",
            ]
            for name in files:
                path = dist / name
                path.write_bytes(b"bin")
                path.with_name(name + ".sig").write_text(f"sig-{name}\n", encoding="utf-8")

            platforms = collect(dist, "org/repo", "v1.0.0")

        self.assertEqual(set(platforms), {"windows-x86_64", "linux-x86_64"})
        self.assertEqual(
            platforms["windows-x86_64"],
            {
                "url": "https://github.com/org/repo/releases/download/v1.0.0/ado-planner-v1.0.0-x86_64-setup.exe",
                "signature": "sig-ado-planner-v1.0.0-x86_64-setup.exe",
                "format": "nsis",
            },
        )
        self.assertEqual(platforms["linux-x86_64"]["format"], "appimage")


if __name__ == "__main__":
    unittest.main()
