#!/usr/bin/env python3
"""Render the expected nespro test-ROM frame as PNG, without an NES core.

The frame is reconstructed from the same data the test ROM writes at boot
(`tools/make_test_rom.py`): 8 background palettes, 4 CHR tiles (one checker,
three solid), a nametable of `tile = value & 3` and an attribute table
alternating 0xE4/0x1B. Colours are then packed exactly like the module does
(`nespro/src/nes_frame.rs`: 2C02 table -> RGB565 -> u16), so the PNG shows the
quantisation the watch actually displays.

Outputs (default /workspace/verify/nespro/expected):
  expected-256x240.png       device framebuffer (256x240)
  expected-336x315.png       nearest-neighbour 1.3125x scale (336x315)
  expected-page-336x480.png  full page preview (image at y=56, black below)

Usage:
  python3 tools/make_expected.py            # built-in copy of the ROM tables
  python3 tools/make_expected.py --rom roms/nespro-test.nes
"""
import argparse
import struct
import zlib
from pathlib import Path

W, H = 256, 240
CONTENT_TOP = 56
SCALE_W, SCALE_H = 336, 315
PAGE_W, PAGE_H = 336, 480

# Same table as nespro/src/nes_frame.rs (x-nes examples/window.rs palette).
PALETTE_RGB = [
    0x545454, 0x001E74, 0x080090, 0x440088, 0x7C005C, 0xA4001C, 0xA80000, 0x880000,
    0x5C2800, 0x284400, 0x005400, 0x005030, 0x004444, 0x000000, 0x000000, 0x000000,
    0xB4B4B4, 0x0C54C4, 0x303CD8, 0x742CC4, 0xAC1898, 0xD8004C, 0xDC0800, 0xBC3000,
    0x805000, 0x486800, 0x107800, 0x007444, 0x00686C, 0x000000, 0x000000, 0x000000,
    0xFCFCFC, 0x64B0FC, 0x9090FC, 0xC87CFC, 0xFC74FC, 0xFC74B8, 0xFC7870, 0xFC9838,
    0xF0B800, 0xBCD000, 0x84DC48, 0x58D878, 0x44D0A8, 0x000000, 0x000000, 0x000000,
    0xFCFCFC, 0xC0E4FC, 0xD0D4FC, 0xE8CCFC, 0xFCC8FC, 0xFCC4E0, 0xFCC8B8, 0xFCD4A0,
    0xFCE090, 0xE4EC88, 0xC8F090, 0xA8F0A8, 0xB0ECC8, 0x000000, 0x000000, 0x000000,
]

# Table contents written by tools/make_test_rom.py (used when --rom is absent).
DEFAULT_PALDATA = bytes([
    0x0F, 0x16, 0x2A, 0x12,
    0x0F, 0x27, 0x31, 0x30,
    0x0F, 0x19, 0x29, 0x39,
    0x0F, 0x01, 0x11, 0x21,
    0x0F, 0x16, 0x2A, 0x12,
    0x0F, 0x27, 0x31, 0x30,
    0x0F, 0x19, 0x29, 0x39,
    0x0F, 0x01, 0x11, 0x21,
])
DEFAULT_CHR = bytes([0xAA, 0x55] * 4 + [0x00] * 8 +      # tile 0: 1px checker
                    [0xFF] * 8 + [0x00] * 8 +            # tile 1: solid colour 1
                    [0x00] * 8 + [0xFF] * 8 +            # tile 2: solid colour 2
                    [0xFF] * 8 + [0xFF] * 8)             # tile 3: solid colour 3
DEFAULT_ATTR = bytes([0xE4, 0x1B] * 32)


def read_rom_tables(path):
    """Pull paldata / CHR tiles / attribute bytes out of a 16 KiB PRG image."""
    blob = Path(path).read_bytes()
    if blob[:4] != b'NES\x1a':
        raise SystemExit('not an iNES file: %s' % path)
    prg_off = 16 + 0x0400  # PRG offset 0x0400 (BASE $8000 + DATA_OFF)
    paldata = blob[prg_off:prg_off + 32]
    chrtiles = blob[prg_off + 32:prg_off + 96]
    attr = blob[prg_off + 96:prg_off + 160]
    if len(paldata) != 32 or len(chrtiles) != 64 or len(attr) != 64:
        raise SystemExit('unexpected test ROM layout')
    return paldata, chrtiles, attr


def nametable_tiles():
    """Reproduce the ROM's fill loops: 3 pages of 256, then 192 more entries."""
    values = []
    for _page in range(3):
        for k in range(256):
            values.append((256 - k) & 0xFF)
    for k in range(192):
        values.append((0xC0 - k) & 0xFF)
    assert len(values) == 960
    return [v & 3 for v in values]


def rgb565(rgb):
    r = (rgb >> 16) & 0xFF
    g = (rgb >> 8) & 0xFF
    b = rgb & 0xFF
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def rgb565_to_rgb(value):
    r = ((value >> 11) & 0x1F) << 3
    g = ((value >> 5) & 0x3F) << 2
    b = (value & 0x1F) << 3
    return r, g, b


def build_frame(paldata, chrtiles, attr):
    tiles = nametable_tiles()
    indices = bytearray(W * H)
    for py in range(H):
        tile_y = py // 8
        row = py % 8
        for px in range(W):
            tile_x = px // 8
            entry = tiles[tile_y * 32 + tile_x]
            lo = chrtiles[entry * 16 + row]
            hi = chrtiles[entry * 16 + 8 + row]
            bit = 7 - (px % 8)
            colour = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1)
            if colour == 0:
                indices[py * W + px] = 0x0F
                continue
            cell = (tile_y // 4) * 8 + (tile_x // 4)
            byte = attr[cell]
            shift = ((tile_y % 4) // 2) * 4 + ((tile_x % 4) // 2) * 2
            palette = (byte >> shift) & 3
            indices[py * W + px] = paldata[palette * 4 + colour]
    return indices


def write_png(path, width, height, pixels):
    raw = b''.join(b'\x00' + pixels[y * width * 3:(y + 1) * width * 3]
                   for y in range(height))

    def chunk(tag, data):
        return (struct.pack('>I', len(data)) + tag + data +
                struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF))

    png = (b'\x89PNG\r\n\x1a\n' +
           chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) +
           chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
    Path(path).write_bytes(png)


def quantised_rgb(indices):
    """Palette index -> RGB565 -> 8-bit RGB, matching the device framebuffer."""
    out = bytearray(W * H * 3)
    for i, index in enumerate(indices):
        r, g, b = rgb565_to_rgb(rgb565(PALETTE_RGB[index & 0x3F]))
        out[i * 3:i * 3 + 3] = bytes((r, g, b))
    return out


def scale_nearest(src, sw, sh, dw, dh):
    out = bytearray(dw * dh * 3)
    for y in range(dh):
        sy = y * sh // dh
        for x in range(dw):
            sx = x * sw // dw
            s = (sy * sw + sx) * 3
            d = (y * dw + x) * 3
            out[d:d + 3] = src[s:s + 3]
    return out


def compose_page(image, iw, ih):
    out = bytearray(PAGE_W * PAGE_H * 3)  # black
    for y in range(ih):
        d = ((CONTENT_TOP + y) * PAGE_W) * 3
        s = y * iw * 3
        out[d:d + iw * 3] = image[s:s + iw * 3]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--rom', help='16 KiB PRG test ROM (defaults to built-in tables)')
    ap.add_argument('--out', default='expected')
    args = ap.parse_args()
    paldata, chrtiles, attr = (read_rom_tables(args.rom) if args.rom
                               else (DEFAULT_PALDATA, DEFAULT_CHR, DEFAULT_ATTR))

    indices = build_frame(paldata, chrtiles, attr)
    base = quantised_rgb(indices)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    write_png(out / 'expected-256x240.png', W, H, base)
    write_png(out / 'expected-336x315.png', SCALE_W, SCALE_H,
              scale_nearest(base, W, H, SCALE_W, SCALE_H))
    write_png(out / 'expected-page-336x480.png', PAGE_W, PAGE_H,
              compose_page(scale_nearest(base, W, H, SCALE_W, SCALE_H), SCALE_W, SCALE_H))

    colours = {bytes(base[i:i + 3]) for i in range(0, len(base), 3)}
    print('frame 256x240, %d colours' % len(colours))
    print('wrote %s/expected-256x240.png' % out)
    print('wrote %s/expected-336x315.png' % out)
    print('wrote %s/expected-page-336x480.png' % out)


if __name__ == '__main__':
    main()
