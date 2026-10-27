#!/usr/bin/env python3
"""Generate the self-authored 48x48 ARGB8888 launcher icon (`appicon_nes.bin`).

The output matches the LVGL v9 BIN layout the firmware expects (see
Canopus-App-LyraPlayer/scripts/encode-lvgl-icon.py): a 12-byte header
`<4sHHHH` with magic 0x19 / cf 0x10, then BGRA pixels.
"""
from __future__ import annotations

import struct
from pathlib import Path

SIZE = 48
OUT = Path(__file__).resolve().parent.parent / "nes_band" / "assets" / "appicon_nes.bin"

BACKGROUND = (0x18, 0x18, 0x1C)
BODY = (0x2E, 0x2E, 0x36)
BODY_EDGE = (0x4A, 0x4A, 0x58)
DPAD = (0xE8, 0xE8, 0xE8)
BUTTON_A = (0xE0, 0x4A, 0x4A)
BUTTON_B = (0x4A, 0x8A, 0xE0)
SCREEN = (0x1C, 0x2C, 0x1C)


def build_pixels() -> bytearray:
    pixels = bytearray(SIZE * SIZE * 4)

    def put(x: int, y: int, color: tuple[int, int, int]) -> None:
        if 0 <= x < SIZE and 0 <= y < SIZE:
            offset = (y * SIZE + x) * 4
            red, green, blue = color
            pixels[offset : offset + 4] = bytes((blue, green, red, 0xFF))

    def rect(x0: int, y0: int, x1: int, y1: int, color: tuple[int, int, int]) -> None:
        for y in range(y0, y1):
            for x in range(x0, x1):
                put(x, y, color)

    # Rounded dark plate.
    for y in range(SIZE):
        for x in range(SIZE):
            edge = min(x, y, SIZE - 1 - x, SIZE - 1 - y)
            if edge < 2:
                continue
            if edge < 5 and (x - 2) ** 2 + (y - 2) ** 2 > 46 * 46:
                continue
            put(x, y, BACKGROUND)

    # Console body and its small screen.
    rect(6, 16, 42, 34, BODY)
    rect(6, 16, 42, 18, BODY_EDGE)
    rect(10, 19, 27, 27, SCREEN)

    # D-pad.
    rect(31, 20, 37, 23, DPAD)
    rect(33, 19, 35, 24, DPAD)

    # Two action buttons.
    rect(34, 28, 38, 32, BUTTON_A)
    rect(28, 28, 32, 32, BUTTON_B)

    # Cartridge notch.
    rect(19, 30, 29, 33, BODY_EDGE)
    return pixels


def main() -> int:
    pixels = build_pixels()
    header = struct.pack("<4sHHHH", b"\x19\x10\x00\x00", SIZE, SIZE, SIZE * 4, 0)
    data = header + bytes(pixels)
    assert len(data) == 12 + SIZE * SIZE * 4
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
