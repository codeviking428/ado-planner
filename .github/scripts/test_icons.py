#!/usr/bin/env python3
"""Unit tests for icons.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import struct
import tempfile
import unittest
import zlib
from pathlib import Path

_SCRIPT = Path(__file__).resolve().parent / "icons.py"
_SPEC = importlib.util.spec_from_file_location("icons", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
icons = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(icons)

REPO = Path(__file__).resolve().parents[2]


def make_png(width: int, height: int, rgba: bytes, filt: int = 0) -> bytes:
    def chunk(kind: bytes, body: bytes) -> bytes:
        crc = zlib.crc32(kind + body)
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    stride = width * 4
    raw = b"".join(bytes([filt]) + filtered(rgba, y, stride, filt) for y in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        icons.PNG_SIGNATURE
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def filtered(rgba: bytes, y: int, stride: int, filt: int) -> bytes:
    line = rgba[y * stride : (y + 1) * stride]
    if filt == 0:
        return line
    prev = rgba[(y - 1) * stride : y * stride] if y else bytes(stride)
    out = bytearray(stride)
    for i in range(stride):
        a = line[i - 4] if i >= 4 else 0
        b = prev[i]
        c = prev[i - 4] if i >= 4 else 0
        if filt == 1:
            pred = a
        elif filt == 2:
            pred = b
        elif filt == 3:
            pred = (a + b) >> 1
        else:
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            pred = a if pa <= pb and pa <= pc else b if pb <= pc else c
        out[i] = (line[i] - pred) & 0xFF
    return bytes(out)


def gradient(width: int, height: int) -> bytes:
    return bytes(
        v
        for y in range(height)
        for x in range(width)
        for v in (x * 40 % 256, y * 60 % 256, (x + y) * 25 % 256, 0 if x == 0 else 255)
    )


class DecodeTests(unittest.TestCase):
    def test_every_filter_round_trips(self) -> None:
        pixels = gradient(5, 4)
        for filt in range(5):
            with self.subTest(filt=filt):
                self.assertEqual(icons.decode_rgba(make_png(5, 4, pixels, filt)), (5, 4, pixels))

    def test_rejects_non_rgba(self) -> None:
        png = bytearray(make_png(1, 1, bytes(4)))
        png[25] = 2
        with self.assertRaises(icons.IconError):
            icons.decode_rgba(bytes(png))


class PackTests(unittest.TestCase):
    def pngs(self) -> dict[int, bytes]:
        sizes = {*icons.ICO_SIZES, *(s for _, s in icons.ICNS_TYPES)}
        return {
            s: make_png(s, s, gradient(s, s)) if s < 256 else make_png(s, s, bytes(4 * s * s))
            for s in sizes
        }

    def test_ico_round_trips_with_bmp_small_and_png_large(self) -> None:
        images = icons.read_ico(icons.pack_ico(self.pngs()))
        self.assertEqual([w for w, _, _ in images], list(icons.ICO_SIZES))
        self.assertEqual({fmt for w, _, fmt in images if w < 256}, {"bmp"})
        self.assertEqual([fmt for w, _, fmt in images if w == 256], ["png"])

    def test_ico_bmp_rows_are_bottom_up_bgra_with_transparent_mask(self) -> None:
        pixels = gradient(2, 2)
        dib = icons.ico_bmp_entry(make_png(2, 2, pixels))
        xor = dib[40 : 40 + 16]
        r, g, b, a = pixels[8:12]
        self.assertEqual(xor[:4], bytes((b, g, r, a)), "first DIB row is the bottom image row")
        mask = dib[40 + 16 :]
        self.assertEqual(len(mask), 8)
        self.assertEqual(mask[0], 0x80, "x=0 is fully transparent, x=1 opaque")

    def test_icns_uses_argb_for_16_and_32_and_png_above(self) -> None:
        pngs = self.pngs()
        found = icons.read_icns(icons.pack_icns(pngs))
        self.assertEqual(sorted(found), sorted(k for k, _ in icons.ICNS_TYPES))
        for kind, size in icons.ICNS_TYPES:
            with self.subTest(kind=kind):
                if kind in (b"ic04", b"ic05"):
                    self.assertEqual(found[kind][:4], b"ARGB")
                    self.assertEqual(
                        icons.icns_argb_to_rgba(found[kind], size), icons.decode_rgba(pngs[size])[2]
                    )
                else:
                    self.assertEqual(icons.png_size(found[kind]), (size, size))
        self.assertFalse({b"icp4", b"icp5", b"icp6"} & set(found))

    def test_icns_rle_round_trips_runs_and_literals(self) -> None:
        for data in (b"", b"a", b"ab", b"aaa", b"a" * 131, b"a" * 300 + bytes(range(256)) * 2):
            with self.subTest(n=len(data)):
                self.assertEqual(icons.icns_unrle(icons.icns_rle(data), len(data)), data)
        self.assertEqual(icons.icns_rle(b"\x07" * 130), b"\xff\x07")
        self.assertEqual(icons.icns_rle(b"xyz"), b"\x02xyz")


def make_pe(resources: dict[tuple[int, int], bytes]) -> bytes:
    """A PE32+ image with one .rsrc section holding the given (type, ID) resources."""
    rva, raw_ptr = 0x1000, 0x200
    types = sorted({t for t, _ in resources})
    ids = {t: sorted(i for k, i in resources if k == t) for t in types}
    # Directory tree first, then 16-byte data entries, then the data.
    tree_size = 16 + 8 * len(types) + sum(16 + 8 * len(ids[t]) + 24 * len(ids[t]) for t in types)
    data_entries = tree_size
    blobs_at = data_entries + 16 * len(resources)
    tree = bytearray(struct.pack("<12xHH", 0, len(types)))
    type_dirs = bytearray()
    lang_dirs = bytearray()
    entries = bytearray()
    blobs = bytearray()
    type_dir_at = 16 + 8 * len(types)
    lang_dir_at = type_dir_at + sum(16 + 8 * len(ids[t]) for t in types)
    for t in types:
        tree += struct.pack("<II", t, 0x80000000 | (type_dir_at + len(type_dirs)))
        type_dirs += struct.pack("<12xHH", 0, len(ids[t]))
        for i in ids[t]:
            here = lang_dir_at + len(lang_dirs)
            type_dirs += struct.pack("<II", i, 0x80000000 | here)
            leaf = data_entries + len(entries)
            lang_dirs += struct.pack("<12xHH", 0, 1) + struct.pack("<II", 1033, leaf)
            entries += struct.pack(
                "<IIII", rva + blobs_at + len(blobs), len(resources[(t, i)]), 0, 0
            )
            blobs += resources[(t, i)]
    section = bytes(tree + type_dirs + lang_dirs + entries + blobs)
    assert len(tree + type_dirs + lang_dirs) == tree_size
    opt = bytearray(240)
    opt[0:2] = struct.pack("<H", 0x20B)
    opt[112 + 16 : 112 + 24] = struct.pack("<II", rva, len(section))
    coff = struct.pack("<HH12xHH", 0x8664, 1, len(opt), 0x22)
    header = struct.pack("<8sIIII16x", b".rsrc", len(section), rva, len(section), raw_ptr)
    image = bytearray(0x40)
    image[0:2] = b"MZ"
    image[0x3C:0x40] = struct.pack("<I", 0x40)
    image += b"PE\0\0" + coff + opt + header
    image += bytes(raw_ptr - len(image))
    return bytes(image + section)


def group_icon(ico: bytes, ids: list[int]) -> bytes:
    count = struct.unpack("<H", ico[4:6])[0]
    out = bytearray(ico[:6])
    for i in range(count):
        out += ico[6 + 16 * i : 6 + 16 * i + 12] + struct.pack("<H", ids[i])
    return bytes(out)


class ExeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.ico_path = REPO / icons.ICO
        self.ico = self.ico_path.read_bytes()
        self.blobs = icons.ico_blobs(self.ico)

    def exe(self, resources: dict[tuple[int, int], bytes]) -> Path:
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        path = tmp / "app.exe"
        path.write_bytes(make_pe(resources))
        return path

    def icon_resources(self, group_id: int, blobs: list[bytes]) -> dict[tuple[int, int], bytes]:
        ids = list(range(1, len(blobs) + 1))
        res = {(icons.RT_ICON, i): b for i, b in zip(ids, blobs)}
        res[(icons.RT_GROUP_ICON, group_id)] = group_icon(self.ico, ids)
        return res

    def test_matching_icon_group_passes(self) -> None:
        icons.check_exe(self.exe(self.icon_resources(1, self.blobs)), self.ico_path)

    def test_icon_under_another_id_fails(self) -> None:
        with self.assertRaisesRegex(icons.IconError, r"no icon group 1 \(found \[2\]\)"):
            icons.check_exe(self.exe(self.icon_resources(2, self.blobs)), self.ico_path)

    def test_nsis_group_in_reverse_order_passes(self) -> None:
        exe = self.exe(self.icon_resources(103, self.blobs[::-1]))
        icons.check_exe(exe, self.ico_path, group_id=103)

    def test_stale_image_fails(self) -> None:
        first = bytearray(self.blobs[0])
        first[100] ^= 0xFF
        stale = [bytes(first), *self.blobs[1:]]
        with self.assertRaisesRegex(icons.IconError, "does not match"):
            icons.check_exe(self.exe(self.icon_resources(1, stale)), self.ico_path)

    def test_not_a_pe_fails(self) -> None:
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        (tmp / "x.exe").write_bytes(b"\x7fELF" + bytes(64))
        with self.assertRaisesRegex(icons.IconError, "not a PE"):
            icons.check_exe(tmp / "x.exe", self.ico_path)


class AppDirTests(unittest.TestCase):
    def appdir(self, desktop: str = "Icon=ado-planner\n", svg: bool = True) -> Path:
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        (tmp / "ado-planner.desktop").write_text(f"[Desktop Entry]\nName=ADO Planner\n{desktop}")
        hicolor = tmp / "usr/share/icons/hicolor"
        for s in icons.PNG_SIZES:
            (hicolor / f"{s}x{s}/apps").mkdir(parents=True)
            shutil.copy(REPO / icons.png_path(s), hicolor / f"{s}x{s}/apps/ado-planner.png")
        shutil.copy(REPO / icons.png_path(512), tmp / ".DirIcon")
        if svg:
            (hicolor / "scalable/apps").mkdir(parents=True)
            shutil.copy(REPO / icons.SVG, hicolor / "scalable/apps/ado-planner.svg")
        return tmp

    def test_complete_appdir_passes(self) -> None:
        icons.check_appdir(self.appdir(), REPO, require_svg=True)
        icons.check_appdir(
            self.appdir("Icon=ado-planner\nStartupWMClass=ado-planner\n", svg=False),
            REPO,
            require_svg=False,
        )

    def test_missing_svg_fails_only_when_required(self) -> None:
        appdir = self.appdir(svg=False)
        icons.check_appdir(appdir, REPO, require_svg=False)
        with self.assertRaisesRegex(icons.IconError, "scalable/apps/ado-planner.svg is missing"):
            icons.check_appdir(appdir, REPO, require_svg=True)

    def test_stale_png_fails(self) -> None:
        appdir = self.appdir()
        shutil.copy(
            REPO / icons.png_path(32), appdir / "usr/share/icons/hicolor/16x16/apps/ado-planner.png"
        )
        with self.assertRaisesRegex(icons.IconError, "16x16/apps/ado-planner.png differs"):
            icons.check_appdir(appdir, REPO, require_svg=True)

    def test_mismatched_wm_class_fails(self) -> None:
        appdir = self.appdir("Icon=ado-planner\nStartupWMClass=ADO Planner\n")
        with self.assertRaisesRegex(icons.IconError, "StartupWMClass"):
            icons.check_appdir(appdir, REPO, require_svg=True)

    def test_icon_path_instead_of_name_fails(self) -> None:
        with self.assertRaisesRegex(icons.IconError, "bare icon theme name"):
            icons.check_appdir(self.appdir("Icon=ado-planner.png\n"), REPO, require_svg=True)


class SvgTests(unittest.TestCase):
    def check(self, body: str) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "x.svg"
            path.write_text(
                f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">{body}</svg>'
            )
            icons.check_svg(path)

    def test_gradients_and_shapes_pass(self) -> None:
        self.check('<defs><linearGradient id="g"/></defs><rect fill="url(#g)"/><use href="#g"/>')

    def test_embedded_raster_fails(self) -> None:
        for body in (
            '<image href="data:image/png;base64,AAAA"/>',
            '<use xlink:href="other.svg#x"/>',
            '<rect style="fill:url(data:image/png;base64,AAAA)"/>',
            "<foreignObject/>",
        ):
            with self.subTest(body=body), self.assertRaises(icons.IconError):
                self.check(body)


class RepoTests(unittest.TestCase):
    def test_committed_icons_pass(self) -> None:
        self.assertEqual(icons.check(REPO), [])

    def copy(self) -> Path:
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        for rel in [icons.SVG, icons.MANIFEST, *icons.outputs()]:
            (tmp / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / rel, tmp / rel)
        return tmp

    def test_edited_svg_without_regenerating_fails(self) -> None:
        root = self.copy()
        svg = root / icons.SVG
        svg.write_text(svg.read_text().replace("#F8FAFC", "#FFFFFF"))
        self.assertTrue(any("SVG changed" in e for e in icons.check(root)))

    def test_hand_edited_output_fails(self) -> None:
        root = self.copy()
        out = root / icons.png_path(16)
        out.write_bytes(out.read_bytes() + b"\0")
        self.assertTrue(any("does not match" in e for e in icons.check(root)))

    def test_wrong_size_and_missing_outputs_fail(self) -> None:
        root = self.copy()
        shutil.copy(root / icons.png_path(32), root / icons.png_path(16))
        (root / icons.ICNS).unlink()
        errors = icons.check(root)
        self.assertTrue(
            any(e.startswith("assets/icons/png/16x16.png: is 32x32") for e in errors), errors
        )
        self.assertTrue(any(e.startswith("assets/icons/ado-planner.icns") for e in errors), errors)

    def test_manifest_lists_every_output(self) -> None:
        data = json.loads((REPO / icons.MANIFEST).read_text())
        self.assertEqual(set(data["outputs"]), {p.as_posix() for p in icons.outputs()})


if __name__ == "__main__":
    unittest.main()
