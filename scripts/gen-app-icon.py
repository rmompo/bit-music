#!/usr/bin/env python3
"""Builds the application icon (bm.exe and bm-gui.exe) from a capture of the Phosphor
`file-audio` glyph (no image libraries needed).

  1. Draw the glyph large, white on black, and capture it:
       BM_GUI_ICON=1 BM_GUI_SCREENSHOT=/tmp/icon.ppm target/debug/bm-gui
  2. Run:  scripts/gen-app-icon.py /tmp/icon.ppm

It writes, in assets/ (shared by both executables):
  icon.ico       several sizes (PNG inside), embedded in the Windows .exe
  icon-128.rgba  raw 128x128 RGBA, used for the window and taskbar icon

The icon is the glyph in white on a rounded blue tile, so it reads on light
and dark backgrounds alike.
"""
import struct
import sys
import zlib
from pathlib import Path

TILE = (33, 105, 186)        # background tile
SIZES = (16, 24, 32, 48, 64, 128, 256)
GLYPH_FRACTION = 0.64        # glyph height as a share of the tile
RADIUS_FRACTION = 0.22       # corner radius as a share of the tile
OUT = Path(__file__).resolve().parent.parent / "assets"


def read_ppm(path):
    data = Path(path).read_bytes()
    _, dims, _, pixels = data.split(b"\n", 3)
    w, h = map(int, dims.split())
    return w, h, pixels


def glyph_mask(path):
    """The glyph's coverage (0..1), cropped to its bounding box."""
    w, h, px = read_ppm(path)
    rows = [[px[(y * w + x) * 3] / 255.0 for x in range(w)] for y in range(h)]
    ys = [y for y in range(h) if max(rows[y]) > 0.12]
    xs = [x for x in range(w) if any(rows[y][x] > 0.12 for y in ys)]
    y0, y1, x0, x1 = ys[0], ys[-1] + 1, xs[0], xs[-1] + 1
    return [row[x0:x1] for row in rows[y0:y1]]


def downsample(mask, out_w, out_h):
    """Area-average a coverage mask down to out_w x out_h."""
    src_h, src_w = len(mask), len(mask[0])
    out = []
    for oy in range(out_h):
        ya, yb = oy * src_h // out_h, max((oy + 1) * src_h // out_h, oy * src_h // out_h + 1)
        row = []
        for ox in range(out_w):
            xa, xb = ox * src_w // out_w, max((ox + 1) * src_w // out_w, ox * src_w // out_w + 1)
            total = count = 0
            for y in range(ya, yb):
                line = mask[y]
                total += sum(line[xa:xb])
                count += xb - xa
            row.append(total / count)
        out.append(row)
    return out


def tile_coverage(size, x, y):
    """Coverage of the rounded tile at pixel (x, y), antialiased (4x4)."""
    r = RADIUS_FRACTION * size
    hits = 0
    for sy in range(4):
        for sx in range(4):
            px, py = x + (sx + 0.5) / 4, y + (sy + 0.5) / 4
            cx = min(max(px, r), size - r)
            cy = min(max(py, r), size - r)
            if (px - cx) ** 2 + (py - cy) ** 2 <= r * r:
                hits += 1
    return hits / 16


def render(size, mask):
    src_h, src_w = len(mask), len(mask[0])
    gh = max(1, round(size * GLYPH_FRACTION))
    gw = max(1, round(gh * src_w / src_h))
    glyph = downsample(mask, gw, gh)
    # Thin strokes vanish at small sizes: thicken them a little there.
    boost = 2.2 if size <= 32 else 1.5 if size <= 48 else 1.0
    ox, oy = (size - gw) // 2, (size - gh) // 2
    rgba = bytearray()
    for y in range(size):
        for x in range(size):
            a = tile_coverage(size, x, y)
            g = 0.0
            if oy <= y < oy + gh and ox <= x < ox + gw:
                g = min(1.0, glyph[y - oy][x - ox] * boost)
            rgba += bytes(round(c * (1 - g) + 255 * g) for c in TILE)
            rgba.append(round(a * 255))
    return bytes(rgba)


def png(size, rgba):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(images):
    """An .ico with PNG images (Windows Vista and later)."""
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for size, data in images:
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset + len(blobs))
        blobs += data
    return head + entries + blobs


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    mask = glyph_mask(sys.argv[1])
    images, rgba128 = [], None
    for size in SIZES:
        rgba = render(size, mask)
        images.append((size, png(size, rgba)))
        if size == 128:
            rgba128 = rgba
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "icon.ico").write_bytes(ico(images))
    (OUT / "icon-128.rgba").write_bytes(rgba128)
    (OUT / "icon-256.png").write_bytes(images[-1][1])
    print(f"wrote {OUT}/icon.ico, icon-128.rgba and icon-256.png")


main()
