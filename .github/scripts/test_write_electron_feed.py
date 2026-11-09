#!/usr/bin/env python3
"""Unit tests for write-electron-feed.py."""

from __future__ import annotations

import base64
import hashlib
import importlib.util
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

_SCRIPT = Path(__file__).resolve().parent / "write-electron-feed.py"
_SPEC = importlib.util.spec_from_file_location("write_electron_feed", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)

NOW = datetime(2026, 11, 9, 20, 15, 30, 123456, tzinfo=timezone.utc)


def b64sha512(data: bytes) -> str:
    return base64.b64encode(hashlib.sha512(data).digest()).decode("ascii")


class WriteFeedsTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dist = Path(self._tmp.name)
        self.exe = b"MZ rust installer"
        self.appimage = b"\x7fELF rust appimage"
        (self.dist / "ado-planner-v1.0.0-x86_64-setup.exe").write_bytes(self.exe)
        (self.dist / "ado-planner-v1.0.0-x86_64.AppImage").write_bytes(self.appimage)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def write(self) -> list[Path]:
        return _MOD.write_feeds(self.dist, "v1.0.0", "1.0.0", self.dist, NOW)

    def test_windows_feed_points_at_the_rust_installer_with_admin_rights(self) -> None:
        self.write()
        sha = b64sha512(self.exe)
        self.assertEqual(
            (self.dist / "latest.yml").read_text(),
            "version: 1.0.0\n"
            "files:\n"
            "  - url: ado-planner-v1.0.0-x86_64-setup.exe\n"
            f"    sha512: {sha}\n"
            f"    size: {len(self.exe)}\n"
            "    isAdminRightsRequired: true\n"
            "path: ado-planner-v1.0.0-x86_64-setup.exe\n"
            f"sha512: {sha}\n"
            "releaseDate: '2026-11-09T20:15:30.123Z'\n",
        )

    def test_linux_feed_points_at_the_rust_appimage(self) -> None:
        self.write()
        sha = b64sha512(self.appimage)
        self.assertEqual(
            (self.dist / "latest-linux.yml").read_text(),
            "version: 1.0.0\n"
            "files:\n"
            "  - url: ado-planner-v1.0.0-x86_64.AppImage\n"
            f"    sha512: {sha}\n"
            f"    size: {len(self.appimage)}\n"
            "path: ado-planner-v1.0.0-x86_64.AppImage\n"
            f"sha512: {sha}\n"
            "releaseDate: '2026-11-09T20:15:30.123Z'\n",
        )

    def test_missing_installer_is_an_error(self) -> None:
        (self.dist / "ado-planner-v1.0.0-x86_64.AppImage").unlink()
        with self.assertRaisesRegex(_MOD.FeedError, "AppImage is missing"):
            self.write()

    def test_tag_must_match_version(self) -> None:
        with self.assertRaisesRegex(_MOD.FeedError, "does not match"):
            _MOD.write_feeds(self.dist, "v1.0.0", "1.0.1", self.dist, NOW)

    def test_main_returns_nonzero_without_installers(self) -> None:
        with tempfile.TemporaryDirectory() as empty:
            code = _MOD.main(["--dist", empty, "--tag", "v1.0.0", "--version", "1.0.0"])
        self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
