#!/usr/bin/env python3
"""Unit tests for check-pe-subsystem.py."""

from __future__ import annotations

import importlib.util
import struct
import tempfile
import unittest
from pathlib import Path

_SCRIPT = Path(__file__).resolve().parent / "check-pe-subsystem.py"
_SPEC = importlib.util.spec_from_file_location("check_pe_subsystem", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)
subsystem = _MOD.subsystem


def fake_pe(value: int, pe_offset: int = 0x80) -> bytes:
    data = bytearray(pe_offset + 4 + 20 + 96)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, pe_offset)
    data[pe_offset : pe_offset + 4] = b"PE\0\0"
    struct.pack_into("<H", data, pe_offset + 4 + 20, 0x20B)
    struct.pack_into("<H", data, pe_offset + 4 + 20 + 68, value)
    return bytes(data)


class SubsystemTests(unittest.TestCase):
    def read(self, data: bytes) -> int:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "app.exe"
            path.write_bytes(data)
            return subsystem(path)

    def test_gui_and_console(self) -> None:
        self.assertEqual(self.read(fake_pe(2)), 2)
        self.assertEqual(self.read(fake_pe(3)), 3)

    def test_follows_e_lfanew(self) -> None:
        self.assertEqual(self.read(fake_pe(2, pe_offset=0x100)), 2)

    def test_rejects_non_pe(self) -> None:
        with self.assertRaises(ValueError):
            self.read(b"\x7fELF" + bytes(200))
        broken = bytearray(fake_pe(2))
        broken[0x80:0x84] = b"XXXX"
        with self.assertRaises(ValueError):
            self.read(bytes(broken))


if __name__ == "__main__":
    unittest.main()
