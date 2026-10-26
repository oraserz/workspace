#!/usr/bin/env python3
"""Convert a binary PPM (P6) to PNG using only the standard library.

Usage: ppm_to_png.py in.ppm out.png
"""
import struct
import sys
import zlib
from pathlib import Path


def read_ppm(path: Path):
    data = path.read_bytes()
    if not data.startswith(b"P6\n"):
        raise SystemExit("not a binary PPM (P6)")
    size_line, rest = data[3:].split(b"\n", 1)
    width, height = (int(v) for v in size_line.split())
    maxval, pixels = rest.split(b"\n", 1)
    if int(maxval) != 255:
        raise SystemExit("only 8-bit PPM supported")
    if len(pixels) != width * height * 3:
        raise SystemExit("truncated pixel data")
    return width, height, pixels


def write_png(path: Path, width: int, height: int, pixels: bytes) -> None:
    raw = bytearray()
    stride = width * 3
    for y in range(height):
        raw.append(0)  # filter type 0
        raw.extend(pixels[y * stride : (y + 1) * stride])

    def chunk(tag: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    width, height, pixels = read_ppm(Path(sys.argv[1]))
    write_png(Path(sys.argv[2]), width, height, pixels)
    print(f"wrote {sys.argv[2]} ({width}x{height})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
