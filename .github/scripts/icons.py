#!/usr/bin/env python3
"""Build and check the ADO Planner app icons from assets/icons/ado-planner.svg.

    python3 .github/scripts/icons.py generate   # needs rsvg-convert (librsvg2-bin)
    python3 .github/scripts/icons.py check      # stdlib only; CI runs this
    python3 .github/scripts/icons.py check-exe target/release/ado-planner.exe
    python3 .github/scripts/icons.py check-exe --group-id 103 setup.exe   # NSIS
    python3 .github/scripts/icons.py check-appdir squashfs-root [--require-svg]

generate renders every size straight from the SVG (no downscaling), packs the
Windows .ico and macOS .icns, copies the Electron runtime icon, and records the
SHA-256 of the SVG and of every output in assets/icons/manifest.json. check
fails if the SVG embeds rasters or external references, if an output is
missing, malformed, or the wrong size, or if any hash in the manifest no longer
matches, which means someone edited the SVG without regenerating or edited an
output by hand. check-exe fails unless the Windows executable's icon group 1,
the one GPUI, Explorer, and shortcuts load, holds exactly the images in
assets/icons/ado-planner.ico; NSIS installers keep their icon in group 103. check-appdir fails unless an extracted AppImage
installs those PNGs (and the SVG, if asked) under the desktop file's Icon= name.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ICONS = Path("assets/icons")
SVG = ICONS / "ado-planner.svg"
MANIFEST = ICONS / "manifest.json"
ICO = ICONS / "ado-planner.ico"
ICNS = ICONS / "ado-planner.icns"
PNG_DIR = ICONS / "png"
ELECTRON_RUNTIME_PNG = Path("resources/icon.png")

# freedesktop hicolor sizes. cargo-packager installs each PNG under
# usr/share/icons/hicolor/<w>x<h>/apps/ado-planner.png by its pixel size, and
# electron-builder needs the size in the file name.
PNG_SIZES = (16, 24, 32, 48, 64, 128, 256, 512)
# 16/32/48/256 are the classic shell sizes; 20/24/40/64 cover 125% to 200%
# scaling of the 16 and 32 px title bar and taskbar slots.
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
# (OSType, pixel size), the set iconutil writes. IconServices draws PNG in
# icp4/icp5 as noise in some views, so 16 and 32 px use ARGB in ic04/ic05.
ICNS_ARGB_TYPES = ((b"ic04", 16), (b"ic05", 32))
ICNS_TYPES = (
    *ICNS_ARGB_TYPES,
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
)
ELECTRON_RUNTIME_SIZE = 512

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
SVG_NS = "{http://www.w3.org/2000/svg}"
XLINK_HREF = "{http://www.w3.org/1999/xlink}href"


class IconError(Exception):
    pass


def png_path(size: int) -> Path:
    return PNG_DIR / f"{size}x{size}.png"


def outputs() -> list[Path]:
    return [*(png_path(s) for s in PNG_SIZES), ICO, ICNS, ELECTRON_RUNTIME_PNG]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def png_size(data: bytes) -> tuple[int, int]:
    if data[:8] != PNG_SIGNATURE or data[12:16] != b"IHDR":
        raise IconError("not a PNG")
    width, height = struct.unpack(">II", data[16:24])
    return width, height


def decode_rgba(data: bytes) -> tuple[int, int, bytes]:
    """Decode an 8-bit RGBA, non-interlaced PNG, which is what rsvg-convert writes."""
    width, height = png_size(data)
    bit_depth, color_type, _, _, interlace = data[24:29]
    if (bit_depth, color_type, interlace) != (8, 6, 0):
        raise IconError(f"expected 8-bit RGBA non-interlaced PNG, got {bit_depth=} {color_type=}")
    idat = bytearray()
    pos = 8
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        if kind == b"IDAT":
            idat += data[pos + 8 : pos + 8 + length]
        pos += 12 + length
    raw = zlib.decompress(bytes(idat))
    stride = width * 4
    out = bytearray(stride * height)
    prev = bytearray(stride)
    for y in range(height):
        filt = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b = prev[i]
            c = prev[i - 4] if i >= 4 else 0
            if filt == 1:
                line[i] = (line[i] + a) & 0xFF
            elif filt == 2:
                line[i] = (line[i] + b) & 0xFF
            elif filt == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 0xFF
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else b if pb <= pc else c
                line[i] = (line[i] + pred) & 0xFF
            elif filt != 0:
                raise IconError(f"bad PNG filter {filt}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, bytes(out)


def ico_bmp_entry(png: bytes) -> bytes:
    """32-bit BGRA DIB with an AND mask, the format every icon consumer reads."""
    width, height, rgba = decode_rgba(png)
    xor = bytearray()
    mask_stride = ((width + 31) // 32) * 4
    mask = bytearray()
    for y in reversed(range(height)):
        row = rgba[y * width * 4 : (y + 1) * width * 4]
        bits = bytearray(mask_stride)
        for x in range(width):
            r, g, b, alpha = row[x * 4 : x * 4 + 4]
            xor += bytes((b, g, r, alpha))
            if alpha == 0:
                bits[x // 8] |= 0x80 >> (x % 8)
        mask += bits
    header = struct.pack(
        "<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0, len(xor) + len(mask), 0, 0, 0, 0
    )
    return header + bytes(xor) + bytes(mask)


def pack_ico(pngs: dict[int, bytes]) -> bytes:
    images = [
        (size, pngs[size] if size >= 256 else ico_bmp_entry(pngs[size])) for size in ICO_SIZES
    ]
    offset = 6 + 16 * len(images)
    head = struct.pack("<HHH", 0, 1, len(images))
    entries = b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return head + entries + b"".join(blob for _, blob in images)


def icns_rle(channel: bytes) -> bytes:
    """The icns PackBits variant: 0..127 copies n+1 bytes, 128..255 repeats one n-125 times."""
    out = bytearray()
    i, n = 0, len(channel)
    while i < n:
        run = 1
        while i + run < n and run < 130 and channel[i + run] == channel[i]:
            run += 1
        if run >= 3:
            out += bytes((0x80 + run - 3, channel[i]))
            i += run
            continue
        start = i
        while i < n and i - start < 128:
            if i + 2 < n and channel[i] == channel[i + 1] == channel[i + 2]:
                break
            i += 1
        out.append(i - start - 1)
        out += channel[start:i]
    return bytes(out)


def icns_unrle(data: bytes, length: int) -> bytes:
    out = bytearray()
    i = 0
    while len(out) < length:
        if i >= len(data):
            raise IconError("icns RLE data ends early")
        c = data[i]
        if c < 0x80:
            out += data[i + 1 : i + 2 + c]
            i += 2 + c
        else:
            out += data[i + 1 : i + 2] * (c - 125)
            i += 2
    if len(out) != length or i != len(data):
        raise IconError("icns RLE data has the wrong length")
    return bytes(out)


def icns_argb(png: bytes) -> bytes:
    """ic04/ic05 payload: "ARGB", then the A, R, G, and B planes, each RLE-compressed."""
    _, _, rgba = decode_rgba(png)
    planes = (rgba[3::4], rgba[0::4], rgba[1::4], rgba[2::4])
    return b"ARGB" + b"".join(icns_rle(plane) for plane in planes)


def icns_argb_to_rgba(payload: bytes, size: int) -> bytes:
    if payload[:4] != b"ARGB":
        raise IconError("ARGB icns entry lacks its ARGB header")
    planes = icns_unrle(payload[4:], 4 * size * size)
    n = size * size
    a, r, g, b = (planes[k * n : (k + 1) * n] for k in range(4))
    return bytes(v for px in zip(r, g, b, a) for v in px)


def pack_icns(pngs: dict[int, bytes]) -> bytes:
    argb = dict(ICNS_ARGB_TYPES)
    body = b""
    for kind, size in ICNS_TYPES:
        payload = icns_argb(pngs[size]) if kind in argb else pngs[size]
        body += kind + struct.pack(">I", 8 + len(payload)) + payload
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def read_ico(data: bytes) -> list[tuple[int, int, str]]:
    """Return (width, height, "png" | "bmp") for each image in an .ico."""
    reserved, kind, count = struct.unpack("<HHH", data[:6])
    if reserved != 0 or kind != 1 or count == 0:
        raise IconError("not an .ico")
    images = []
    for i in range(count):
        w, h, _, _, _, bpp, length, offset = struct.unpack(
            "<BBBBHHII", data[6 + 16 * i : 22 + 16 * i]
        )
        blob = data[offset : offset + length]
        if len(blob) != length:
            raise IconError(f"ico image {i} is truncated")
        w, h = w or 256, h or 256
        if blob[:8] == PNG_SIGNATURE:
            if png_size(blob) != (w, h):
                raise IconError(f"ico image {i} PNG is {png_size(blob)}, directory says {w}x{h}")
            images.append((w, h, "png"))
        else:
            size, bw, bh, _, bits = struct.unpack("<IiiHH", blob[:16])
            if size != 40 or (bw, bh) != (w, 2 * h) or bits != 32 or bpp != 32:
                raise IconError(f"ico image {i} has a bad DIB header")
            images.append((w, h, "bmp"))
    return images


def ico_blobs(data: bytes) -> list[bytes]:
    count = struct.unpack("<H", data[4:6])[0]
    blobs = []
    for i in range(count):
        length, offset = struct.unpack("<II", data[14 + 16 * i : 22 + 16 * i])
        blobs.append(data[offset : offset + length])
    return blobs


RT_ICON = 3
RT_GROUP_ICON = 14
APP_ICON_ID = 1


def pe_resources(data: bytes) -> dict[tuple[int, int], bytes]:
    """Map (resource type, integer ID) to the first language's data in a PE file."""
    if data[:2] != b"MZ":
        raise IconError("not a PE executable")
    (pe,) = struct.unpack("<I", data[0x3C:0x40])
    if data[pe : pe + 4] != b"PE\0\0":
        raise IconError("not a PE executable")
    sections, opt_size = struct.unpack("<H12xH", data[pe + 6 : pe + 22])
    opt = pe + 24
    magic = struct.unpack("<H", data[opt : opt + 2])[0]
    dirs = opt + {0x10B: 96, 0x20B: 112}[magic]
    rsrc_rva, rsrc_size = struct.unpack("<II", data[dirs + 16 : dirs + 24])
    if rsrc_size == 0:
        raise IconError("executable has no resources")
    table = []
    for i in range(sections):
        s = opt + opt_size + 40 * i
        vsize, vaddr, raw_size, raw_ptr = struct.unpack("<IIII", data[s + 8 : s + 24])
        table.append((vaddr, max(vsize, raw_size), raw_ptr))

    def offset(rva: int) -> int:
        for vaddr, size, raw_ptr in table:
            if vaddr <= rva < vaddr + size:
                return rva - vaddr + raw_ptr
        raise IconError(f"RVA {rva:#x} is outside every section")

    base = offset(rsrc_rva)

    def entries(dir_off: int) -> list[tuple[int, int]]:
        named, ids = struct.unpack("<HH", data[base + dir_off + 12 : base + dir_off + 16])
        first = base + dir_off + 16
        return [
            struct.unpack("<II", data[first + 8 * i : first + 8 * i + 8])
            for i in range(named + ids)
        ]

    found = {}
    for kind, sub in entries(0):
        if kind & 0x80000000 or kind not in (RT_ICON, RT_GROUP_ICON):
            continue
        for ident, langs in entries(sub & 0x7FFFFFFF):
            if ident & 0x80000000:
                continue
            _, leaf = entries(langs & 0x7FFFFFFF)[0]
            rva, size = struct.unpack("<II", data[base + leaf : base + leaf + 8])
            found[(kind, ident)] = data[offset(rva) : offset(rva) + size]
    return found


def check_exe(exe: Path, ico: Path, group_id: int = APP_ICON_ID) -> None:
    res = pe_resources(exe.read_bytes())
    group = res.get((RT_GROUP_ICON, group_id))
    if group is None:
        found = sorted(i for k, i in res if k == RT_GROUP_ICON)
        raise IconError(f"no icon group {group_id} (found {found})")
    want = ico_blobs(ico.read_bytes())
    count = struct.unpack("<H", group[4:6])[0]
    got = []
    for i in range(count):
        (ident,) = struct.unpack("<H", group[6 + 14 * i + 12 : 6 + 14 * i + 14])
        got.append(res.get((RT_ICON, ident)) or b"")
    # makensis stores the images largest first.
    if sorted(got) != sorted(want):
        raise IconError(f"icon group {group_id} does not match {ico.as_posix()}")


def desktop_entry(path: Path) -> dict[str, str]:
    entry: dict[str, str] = {}
    section = None
    for line in path.read_text().splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line
        elif section == "[Desktop Entry]" and "=" in line and not line.startswith("#"):
            key, value = line.split("=", 1)
            entry.setdefault(key.strip(), value.strip())
    return entry


def check_appdir(appdir: Path, root: Path, require_svg: bool) -> None:
    """Check an extracted AppImage (squashfs-root) ships the generated icons."""
    desktops = sorted(appdir.glob("*.desktop"))
    if len(desktops) != 1:
        raise IconError(f"want one top-level .desktop file, found {[d.name for d in desktops]}")
    entry = desktop_entry(desktops[0])
    name = entry.get("Icon", "")
    if not name or "/" in name or "." in name:
        raise IconError(f"{desktops[0].name}: Icon={name!r} must be a bare icon theme name")
    wm_class = entry.get("StartupWMClass")
    if wm_class is not None and wm_class != desktops[0].stem:
        raise IconError(
            f"{desktops[0].name}: StartupWMClass={wm_class!r} does not match the file name, "
            "so docks will not pair the window with this launcher"
        )
    hicolor = appdir / "usr/share/icons/hicolor"
    expected = {hicolor / f"{s}x{s}/apps/{name}.png": root / png_path(s) for s in PNG_SIZES}
    expected[appdir / ".DirIcon"] = root / png_path(max(PNG_SIZES))
    svg = hicolor / f"scalable/apps/{name}.svg"
    if require_svg or svg.exists():
        expected[svg] = root / SVG
    for shipped, source in expected.items():
        rel = shipped.relative_to(appdir).as_posix()
        if not shipped.is_file():
            raise IconError(f"{rel} is missing")
        if shipped.read_bytes() != source.read_bytes():
            raise IconError(f"{rel} differs from {source.relative_to(root).as_posix()}")


def read_icns(data: bytes) -> dict[bytes, bytes]:
    """Map each icns OSType to its payload."""
    if data[:4] != b"icns" or struct.unpack(">I", data[4:8])[0] != len(data):
        raise IconError("not an .icns")
    found = {}
    pos = 8
    while pos < len(data):
        kind = data[pos : pos + 4]
        (length,) = struct.unpack(">I", data[pos + 4 : pos + 8])
        if length < 8 or pos + length > len(data):
            raise IconError(f"icns entry {kind!r} is truncated")
        found[kind] = data[pos + 8 : pos + length]
        pos += length
    return found


def check_svg(path: Path) -> None:
    root = ET.parse(path).getroot()
    if root.tag != f"{SVG_NS}svg":
        raise IconError(f"{path}: root element is not <svg>")
    for el in root.iter():
        if el.tag in (f"{SVG_NS}image", f"{SVG_NS}foreignObject", f"{SVG_NS}script"):
            raise IconError(f"{path}: <{el.tag.removeprefix(SVG_NS)}> is not allowed")
        for attr in ("href", XLINK_HREF):
            href = el.get(attr)
            if href is not None and not href.startswith("#"):
                raise IconError(f"{path}: external reference {href!r}")
        for value in el.attrib.values():
            if "data:" in value or "url(http" in value:
                raise IconError(f"{path}: embedded or remote resource in {value!r}")


def render(svg: Path, size: int, out: Path) -> bytes:
    subprocess.run(
        [
            "rsvg-convert",
            "--width",
            str(size),
            "--height",
            str(size),
            "--format",
            "png",
            "--output",
            str(out),
            str(svg),
        ],
        check=True,
    )
    data = out.read_bytes()
    if png_size(data) != (size, size):
        raise IconError(f"rsvg-convert wrote {png_size(data)} for {size}px")
    return data


def generate(root: Path) -> None:
    if shutil.which("rsvg-convert") is None:
        raise IconError("rsvg-convert not found; install librsvg2-bin (apt) or librsvg (brew)")
    svg = root / SVG
    check_svg(svg)
    sizes = sorted({*PNG_SIZES, *ICO_SIZES, *(s for _, s in ICNS_TYPES), ELECTRON_RUNTIME_SIZE})
    with tempfile.TemporaryDirectory() as tmp:
        pngs = {s: render(svg, s, Path(tmp) / f"{s}.png") for s in sizes}
    (root / PNG_DIR).mkdir(parents=True, exist_ok=True)
    for size in PNG_SIZES:
        (root / png_path(size)).write_bytes(pngs[size])
    (root / ICO).write_bytes(pack_ico(pngs))
    (root / ICNS).write_bytes(pack_icns(pngs))
    (root / ELECTRON_RUNTIME_PNG).write_bytes(pngs[ELECTRON_RUNTIME_SIZE])
    manifest = {
        "source": {"path": SVG.as_posix(), "sha256": sha256(svg)},
        "outputs": {p.as_posix(): sha256(root / p) for p in outputs()},
    }
    (root / MANIFEST).write_text(json.dumps(manifest, indent=2) + "\n")


def check(root: Path) -> list[str]:
    errors: list[str] = []

    def guard(label: str, fn) -> None:
        try:
            fn()
        except (IconError, OSError, ET.ParseError, struct.error, zlib.error) as e:
            errors.append(f"{label}: {e}")

    guard(SVG.as_posix(), lambda: check_svg(root / SVG))

    for size in PNG_SIZES:
        path = root / png_path(size)

        def png(path: Path = path, size: int = size) -> None:
            got = png_size(path.read_bytes())
            if got != (size, size):
                raise IconError(f"is {got[0]}x{got[1]}")

        guard(png_path(size).as_posix(), png)

    def ico() -> None:
        images = read_ico((root / ICO).read_bytes())
        got = sorted(w for w, h, _ in images if w == h)
        if got != sorted(ICO_SIZES):
            raise IconError(f"sizes {got}, want {sorted(ICO_SIZES)}")
        for w, _, fmt in images:
            if w < 256 and fmt != "bmp":
                raise IconError(f"{w}px entry is {fmt}; small entries must be BMP")

    guard(ICO.as_posix(), ico)

    def icns() -> None:
        found = read_icns((root / ICNS).read_bytes())
        want = [kind for kind, _ in ICNS_TYPES]
        if sorted(found) != sorted(want):
            raise IconError(
                f"types {sorted(k.decode() for k in found)}, want {sorted(k.decode() for k in want)}"
            )
        argb = dict(ICNS_ARGB_TYPES)
        for kind, size in ICNS_TYPES:
            if kind in argb:
                pixels = icns_argb_to_rgba(found[kind], size)
                if pixels != decode_rgba((root / png_path(size)).read_bytes())[2]:
                    raise IconError(
                        f"{kind.decode()} pixels differ from {png_path(size).as_posix()}"
                    )
            elif png_size(found[kind]) != (size, size):
                raise IconError(f"{kind.decode()} is {png_size(found[kind])}, want {size}x{size}")

    guard(ICNS.as_posix(), icns)

    def runtime() -> None:
        got = png_size((root / ELECTRON_RUNTIME_PNG).read_bytes())
        if got != (ELECTRON_RUNTIME_SIZE, ELECTRON_RUNTIME_SIZE):
            raise IconError(f"is {got[0]}x{got[1]}")

    guard(ELECTRON_RUNTIME_PNG.as_posix(), runtime)

    def manifest() -> None:
        data = json.loads((root / MANIFEST).read_text())
        stale = "out of date; run `python3 .github/scripts/icons.py generate`"
        if data["source"] != {"path": SVG.as_posix(), "sha256": sha256(root / SVG)}:
            raise IconError(f"SVG changed since the icons were generated; {stale}")
        want = {p.as_posix() for p in outputs()}
        if set(data["outputs"]) != want:
            raise IconError(f"lists {sorted(data['outputs'])}, want {sorted(want)}; {stale}")
        for rel, digest in data["outputs"].items():
            if (root / rel).exists() and sha256(root / rel) != digest:
                raise IconError(f"{rel} does not match the generated file; {stale}")

    guard(MANIFEST.as_posix(), manifest)
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("command", choices=("generate", "check", "check-exe", "check-appdir"))
    parser.add_argument("path", nargs="?", type=Path, help="exe or extracted AppImage directory")
    parser.add_argument(
        "--group-id", type=int, default=APP_ICON_ID, help="check-exe: icon group resource ID"
    )
    parser.add_argument(
        "--require-svg", action="store_true", help="check-appdir: the scalable SVG must ship"
    )
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_intermixed_args(argv)
    if args.command.startswith("check-") != (args.path is not None):
        parser.error(
            f"{args.command} takes exactly one path argument"
            if args.path is None
            else f"{args.command} takes no path argument"
        )
    try:
        if args.command == "check-exe":
            check_exe(args.path, args.root / ICO, args.group_id)
            print(f"{args.path.as_posix()}: icon group {args.group_id} matches {ICO.as_posix()}")
            return 0
        if args.command == "check-appdir":
            check_appdir(args.path, args.root, args.require_svg)
            print(f"{args.path.as_posix()}: ships the icons from {ICONS.as_posix()}")
            return 0
        if args.command == "generate":
            generate(args.root)
        errors = check(args.root)
    except (IconError, OSError, KeyError, struct.error, subprocess.CalledProcessError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if not errors:
        print(f"icons ok: {len(outputs())} files from {SVG.as_posix()}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
