#!/usr/bin/env python3
"""Write electron-updater feeds that point the old Electron app at the Rust installers.

    write-electron-feed.py --dist dist/rust --tag v1.0.0 --version 1.0.0

Installed Electron builds (releases before 1.0.0) read latest.yml (Windows) and
latest-linux.yml (Linux) from the latest GitHub Release. These feeds list the
Rust NSIS installer and AppImage, so the Electron app's usual update prompt
installs the Rust app. On Windows the Rust installer then removes the Electron
install (crates/app/packaging/windows/installer.nsi); on Linux electron-updater
deletes the old AppImage and starts the new one. See docs/RELEASING.md.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import sys
from datetime import datetime, timezone
from pathlib import Path


class FeedError(Exception):
    pass


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out-dir", type=Path)
    return parser.parse_args(argv)


def sha512_base64(path: Path) -> str:
    digest = hashlib.sha512()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return base64.b64encode(digest.digest()).decode("ascii")


def feed(path: Path, version: str, release_date: str, admin: bool) -> str:
    """One feed in electron-builder's latest.yml layout."""
    sha512 = sha512_base64(path)
    lines = [
        f"version: {version}",
        "files:",
        f"  - url: {path.name}",
        f"    sha512: {sha512}",
        f"    size: {path.stat().st_size}",
    ]
    if admin:
        # electron-updater runs the installer through elevate.exe only when
        # the files[] entry says so; the Rust installer requires admin rights.
        lines.append("    isAdminRightsRequired: true")
    lines += [
        f"path: {path.name}",
        f"sha512: {sha512}",
        f"releaseDate: '{release_date}'",
    ]
    return "\n".join(lines) + "\n"


def write_feeds(dist: Path, tag: str, version: str, out_dir: Path, now: datetime) -> list[Path]:
    if not tag.startswith("v") or tag[1:] != version:
        raise FeedError(f"tag {tag} does not match version {version}")
    release_date = now.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + (
        f"{now.microsecond // 1000:03d}Z"
    )
    targets = [
        ("latest.yml", f"ado-planner-{tag}-x86_64-setup.exe", True),
        ("latest-linux.yml", f"ado-planner-{tag}-x86_64.AppImage", False),
    ]
    written = []
    for feed_name, installer, admin in targets:
        path = dist / installer
        if not path.is_file():
            raise FeedError(f"{path} is missing; the Electron app would find no update")
        out = out_dir / feed_name
        out.write_text(feed(path, version, release_date, admin), encoding="utf-8")
        written.append(out)
    return written


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        written = write_feeds(
            args.dist,
            args.tag,
            args.version,
            args.out_dir or args.dist,
            datetime.now(timezone.utc),
        )
    except FeedError as e:
        print(f"write-electron-feed: {e}", file=sys.stderr)
        return 1
    for path in written:
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
