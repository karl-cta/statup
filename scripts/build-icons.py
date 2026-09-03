#!/usr/bin/env python3
"""Rasterize the Statup mark into the PNG icons browsers still ask for.

static/logo.svg stays the source of truth: modern browsers use it directly.
Safari before 15 and the iOS home screen need PNG, and no rasterizer is
assumed to be installed, so the mark is drawn here from the same geometry
using the standard library only.

Usage: python3 scripts/build-icons.py
"""

import struct
import zlib
from pathlib import Path

INK = (0x1C, 0x1B, 0x18)
SAGE = (0x62, 0x81, 0x41)
WHITE = (0xFF, 0xFF, 0xFF)

# Mark geometry, identical to static/logo.svg (viewBox 12 8 36 48).
BARS = [(12, 8, 48, 16), (12, 16, 20, 32), (40, 32, 48, 48), (12, 48, 48, 56)]
DOT = (44, 24, 4)
ART = (12, 8, 48, 56)  # x0, y0, x1, y1

SS = 4  # supersampling factor, box-downsampled afterwards for clean edges


def draw(size, mark_height, background):
    """Render the mark centered on a square canvas, returning RGBA rows."""
    w = h = size * SS
    art_h = mark_height * SS
    scale = art_h / (ART[3] - ART[1])
    art_w = (ART[2] - ART[0]) * scale
    off_x = (w - art_w) / 2 - ART[0] * scale
    off_y = (h - art_h) / 2 - ART[1] * scale

    bg = background + (255,) if background else (0, 0, 0, 0)
    px = [[bg] * w for _ in range(h)]

    def fill(x0, y0, x1, y1, color):
        for y in range(max(0, int(y0 * scale + off_y)), min(h, int(y1 * scale + off_y))):
            for x in range(max(0, int(x0 * scale + off_x)), min(w, int(x1 * scale + off_x))):
                px[y][x] = color + (255,)

    for x0, y0, x1, y1 in BARS:
        fill(x0, y0, x1, y1, INK)

    cx, cy, r = DOT
    cxp, cyp, rp = cx * scale + off_x, cy * scale + off_y, r * scale
    for y in range(max(0, int(cyp - rp)), min(h, int(cyp + rp) + 1)):
        for x in range(max(0, int(cxp - rp)), min(w, int(cxp + rp) + 1)):
            if (x + 0.5 - cxp) ** 2 + (y + 0.5 - cyp) ** 2 <= rp * rp:
                px[y][x] = SAGE + (255,)

    # Box downsample: averages the supersampled grid, which is what smooths edges.
    out = []
    for y in range(size):
        row = []
        for x in range(size):
            acc = [0, 0, 0, 0]
            for dy in range(SS):
                for dx in range(SS):
                    p = px[y * SS + dy][x * SS + dx]
                    for i in range(4):
                        acc[i] += p[i]
            row.append(tuple(c // (SS * SS) for c in acc))
        out.append(row)
    return out


def write_png(path, rows):
    size = len(rows)
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("4B", *p) for p in row) for row in rows
    )

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    Path(path).write_bytes(png)
    print(f"  {path}  {size}x{size}  {len(png)} octets")


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent / "static"
    # Transparent, drawn large in its box: a favicon is read at a glance.
    write_png(root / "favicon-32.png", draw(32, 30, None))
    # Opaque: iOS composites home screen icons on its own background.
    write_png(root / "apple-touch-icon.png", draw(180, 108, WHITE))
