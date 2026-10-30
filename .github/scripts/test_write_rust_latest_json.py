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
darwin_arch = _MOD.darwin_arch
platform_for = _MOD.platform_for


class DarwinArchTests(unittest.TestCase):
    def test_unlabeled_defaults_to_aarch64(self) -> None:
        self.assertEqual(darwin_arch("ado planner.app.tar.gz"), "aarch64")

    def test_explicit_arm(self) -> None:
        self.assertEqual(darwin_arch("ado-planner_0.2.0_aarch64.app.tar.gz"), "aarch64")
        self.assertEqual(darwin_arch("ado-planner_0.2.0_arm64.dmg"), "aarch64")

    def test_explicit_intel(self) -> None:
        self.assertEqual(darwin_arch("ado-planner_0.2.0_x86_64.app.tar.gz"), "x86_64")
        self.assertEqual(darwin_arch("ado-planner_0.2.0_x64.app.tar.gz"), "x86_64")


class PlatformForTests(unittest.TestCase):
    def test_app_tarball_emits_darwin_and_macos_keys(self) -> None:
        mapped = platform_for(Path("ADO Planner.app.tar.gz"))
        self.assertIsNotNone(mapped)
        assert mapped is not None
        keys, fmt = mapped
        self.assertEqual(fmt, "app")
        self.assertEqual(keys, ["darwin-aarch64", "macos-aarch64"])

    def test_intel_tarball(self) -> None:
        mapped = platform_for(Path("ADO Planner_0.2.0_x86_64.app.tar.gz"))
        self.assertIsNotNone(mapped)
        assert mapped is not None
        keys, fmt = mapped
        self.assertEqual(fmt, "app")
        self.assertEqual(keys, ["darwin-x86_64", "macos-x86_64"])

    def test_dmg_is_not_an_updater_artifact(self) -> None:
        self.assertIsNone(platform_for(Path("ADO Planner.dmg")))
        self.assertIsNone(platform_for(Path("ADO Planner_aarch64.dmg")))


class CollectTests(unittest.TestCase):
    def test_prefers_signed_app_tarball_and_skips_dmg(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp)
            tarball = dist / "ADO Planner.app.tar.gz"
            dmg = dist / "ADO Planner.dmg"
            tarball.write_bytes(b"tar")
            dmg.write_bytes(b"dmg")
            tarball.with_name(tarball.name + ".sig").write_text("sig-tar\n", encoding="utf-8")
            dmg.with_name(dmg.name + ".sig").write_text("sig-dmg\n", encoding="utf-8")

            platforms = collect(dist, "codeviking428/ado-planner", "rust-v0.2.0")

        self.assertEqual(set(platforms), {"darwin-aarch64", "macos-aarch64"})
        for entry in platforms.values():
            self.assertTrue(entry["url"].endswith("ADO%20Planner.app.tar.gz"))
            self.assertEqual(entry["format"], "app")
            self.assertEqual(entry["signature"], "sig-tar")

    def test_full_matrix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp)
            files = {
                "ado-planner-setup.nsis.zip": "nsis",
                "ado-planner.AppImage.tar.gz": "appimage",
                "ADO Planner.app.tar.gz": "mac",
            }
            for name in files:
                path = dist / name
                path.write_bytes(b"bin")
                path.with_name(name + ".sig").write_text(f"sig-{name}", encoding="utf-8")

            platforms = collect(dist, "org/repo", "rust-v1.0.0")

        self.assertEqual(
            set(platforms),
            {
                "windows-x86_64",
                "linux-x86_64",
                "darwin-aarch64",
                "macos-aarch64",
            },
        )
        self.assertEqual(platforms["windows-x86_64"]["format"], "nsis")
        self.assertEqual(platforms["linux-x86_64"]["format"], "appimage")
        self.assertEqual(platforms["darwin-aarch64"]["format"], "app")
        self.assertEqual(platforms["macos-aarch64"], platforms["darwin-aarch64"])


if __name__ == "__main__":
    unittest.main()
