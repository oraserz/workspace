#!/usr/bin/env python3
"""Convert binary PPM (P6) files to PNG using only the standard library."""

import struct
import sys
import zlib
from pathlib import Path


def read_ppm(path):
    data = Path(path).read_bytes()
    # P6 header: magic, width, height, maxval, then raw RGB
    pos = 0
    fields = []
    while len(fields) < 4:
        while data[pos : pos + 1].isspace():
            pos += 1
        start = pos
        while pos < len(data) and not data[pos : pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    pos += 1
    magic, width, height, maxval = fields
    assert magic == b"P6" and maxval == b"255"
    w, h = int(width), int(height)
    pixels = data[pos : pos + w * h * 3]
    return w, h, pixels


def write_png(path, w, h, rgb):
    def chunk(tag, payload):
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    raw = bytearray()
    stride = w * 3
    for y in range(h):
        raw.append(0)
        raw.extend(rgb[y * stride : (y + 1) * stride])
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    Path(path).write_bytes(png)


def main():
    for src in sys.argv[1:]:
        p = Path(src)
        w, h, rgb = read_ppm(p)
        out = p.with_suffix(".png")
        write_png(out, w, h, rgb)
        print(f"{p} -> {out} ({w}x{h})")


if __name__ == "__main__":
    main()
