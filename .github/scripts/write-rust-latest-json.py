#!/usr/bin/env python3
"""Build cargo-packager-updater latest.json from packaged artifacts."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--notes", default="")
    return parser.parse_args()


def signature_for(path: Path) -> str | None:
    sig = path.with_name(path.name + ".sig")
    if not sig.is_file():
        return None
    return sig.read_text(encoding="utf-8").strip() or None


def darwin_arch(name: str) -> str:
    """Arch for a macOS artifact name.

    cargo-packager often omits arch from `.app.tar.gz` names. This workflow
    only packs macOS on `macos-latest` (Apple Silicon), so unlabeled Darwin
    artifacts are aarch64. Explicit x86_64 tokens still map to Intel.
    """
    if any(token in name for token in ("aarch64", "arm64")):
        return "aarch64"
    if any(token in name for token in ("x86_64", "amd64", "x64", "intel")):
        return "x86_64"
    return "aarch64"


def mac_platform_keys(arch: str) -> list[str]:
    # cargo-packager-updater 0.2 README examples use `darwin-*`; the 0.2.3
    # client looks up `macos-{arch}` via get_updater_target(). Emit both.
    return [f"darwin-{arch}", f"macos-{arch}"]


def platform_for(path: Path) -> tuple[list[str], str] | None:
    name = path.name.lower()
    if name.endswith(".sig"):
        return None
    if name.endswith(".exe") or "nsis" in name or name.endswith(".msi"):
        fmt = "nsis" if not name.endswith(".msi") else "wix"
        return ["windows-x86_64"], fmt
    if "appimage" in name:
        return ["linux-x86_64"], "appimage"
    if name.endswith(".app.tar.gz") or name.endswith(".app.tar.gz.zip"):
        return mac_platform_keys(darwin_arch(name)), "app"
    # DMGs are GitHub-release downloads only. cargo-packager-updater's `app`
    # format gunzips a .app tar; advertising a .dmg as `app` would fail install.
    return None


def rank(path: Path) -> int:
    name = path.name.lower()
    if name.endswith(".nsis.zip") or name.endswith(".app.tar.gz") or "appimage.tar.gz" in name:
        return 0
    if name.endswith(".exe") or name.endswith(".appimage"):
        return 1
    return 2


def asset_url(repo: str, tag: str, filename: str) -> str:
    return f"https://github.com/{repo}/releases/download/{tag}/{quote(filename)}"


def collect(dist: Path, repo: str, tag: str) -> dict[str, dict[str, str]]:
    candidates: dict[str, tuple[int, dict[str, str]]] = {}
    for path in sorted(dist.rglob("*")):
        if not path.is_file():
            continue
        mapped = platform_for(path)
        if mapped is None:
            continue
        keys, fmt = mapped
        signature = signature_for(path)
        if signature is None:
            continue
        entry = {
            "url": asset_url(repo, tag, path.name),
            "signature": signature,
            "format": fmt,
        }
        score = rank(path)
        for key in keys:
            previous = candidates.get(key)
            if previous is None or score < previous[0]:
                candidates[key] = (score, entry)
    return {key: entry for key, (_, entry) in sorted(candidates.items())}


def main() -> None:
    args = parse_args()
    payload = {
        "version": args.version.lstrip("v"),
        "notes": args.notes or None,
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": collect(args.dist, args.repo, args.tag),
    }
    args.out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out} with {len(payload['platforms'])} platform(s)")


if __name__ == "__main__":
    main()
