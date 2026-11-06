#!/usr/bin/env python3
"""Check the subsystem in a Windows executable's PE header.

    check-pe-subsystem.py ado-planner.exe gui

`gui` (IMAGE_SUBSYSTEM_WINDOWS_GUI) starts without a console window;
`console` (IMAGE_SUBSYSTEM_WINDOWS_CUI) gets one when launched from Explorer.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

SUBSYSTEMS = {"gui": 2, "console": 3}


def subsystem(path: Path) -> int:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise ValueError(f"{path} is not a PE file (no MZ header)")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError(f"{path} has no PE signature at {pe:#x}")
    optional_header = pe + 4 + 20
    # Subsystem sits at the same offset in PE32 and PE32+ optional headers.
    return struct.unpack_from("<H", data, optional_header + 68)[0]


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[2] not in SUBSYSTEMS:
        print(__doc__, file=sys.stderr)
        return 2
    path, want = Path(sys.argv[1]), sys.argv[2]
    got = subsystem(path)
    names = {v: k for k, v in SUBSYSTEMS.items()}
    print(f"{path.name}: subsystem {got} ({names.get(got, 'other')})")
    if got != SUBSYSTEMS[want]:
        print(f"::error::{path.name} uses subsystem {got}, expected {want} ({SUBSYSTEMS[want]})")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
