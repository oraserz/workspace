#!/usr/bin/env python3
"""Builds a self-authored NROM test cartridge (no copyrighted assets).

ROM contents:
  * 256x240 background split into horizontal bands:
      rows 0-7   solid palette entry 1 ($16)
      rows 8-15  solid palette entry 2 ($2A)
      rows 16-23 solid palette entry 3 ($12)
      rows 24-29 checkerboard of entries 1/2
  * NMI increments the vertical scroll every frame, so consecutive frames
    differ (the bands slide upward).
  * No sprites, no audio.

The ROM is used both by the host exporter and by the device module (embedded
fallback when /data/canopus/nes/rom.nes is absent).
"""

import pathlib
import struct

PRG_BANK = 0x4000
CHR_BANK = 0x2000

# --- 6502 program, assembled by hand -------------------------------------
CODE = bytearray()
labels = {}


def label(name):
    labels[name] = 0xC000 + len(CODE)


def emit(*values):
    CODE.extend(values)


label("reset")
emit(0x78)              # SEI
emit(0xD8)              # CLD
emit(0xA2, 0xFF)        # LDX #$FF
emit(0x9A)              # TXS
emit(0xA9, 0x00)        # LDA #$00
emit(0x8D, 0x00, 0x20)  # STA $2000
emit(0x8D, 0x01, 0x20)  # STA $2001

label("wait1")
emit(0x2C, 0x02, 0x20)  # BIT $2002
emit(0x10, 0xFB)        # BPL wait1
label("wait2")
emit(0x2C, 0x02, 0x20)  # BIT $2002
emit(0x10, 0xFB)        # BPL wait2

# $3F00 palette
emit(0xA9, 0x3F, 0x8D, 0x06, 0x20)  # LDA #$3F / STA $2006
emit(0xA9, 0x00, 0x8D, 0x06, 0x20)  # LDA #$00 / STA $2006
emit(0xA2, 0x00)                    # LDX #$00
label("pal_loop")
emit(0xBD, 0x00, 0x00)              # LDA pal_data,X   (patched below)
emit(0x8D, 0x07, 0x20)              # STA $2007
emit(0xE8)                          # INX
emit(0xE0, 0x20)                    # CPX #$20
emit(0xD0, 0xF5)                    # BNE pal_loop

# nametable $2000
emit(0xA9, 0x20, 0x8D, 0x06, 0x20)  # LDA #$20 / STA $2006
emit(0xA9, 0x00, 0x8D, 0x06, 0x20)  # LDA #$00 / STA $2006
emit(0xA0, 0x00)                    # LDY #$00
label("row_loop")
emit(0xB9, 0x00, 0x00)              # LDA row_tiles,Y  (patched below)
emit(0xA2, 0x20)                    # LDX #$20
label("col_loop")
emit(0x8D, 0x07, 0x20)              # STA $2007
emit(0xCA)                          # DEX
emit(0xD0, 0xFA)                    # BNE col_loop
emit(0xC8)                          # INY
emit(0xC0, 0x1E)                    # CPY #$1E
emit(0xD0, 0xF0)                    # BNE row_loop

# attributes $23C0: 64 zero bytes (BG palette 0 everywhere)
emit(0xA9, 0x23, 0x8D, 0x06, 0x20)  # LDA #$23 / STA $2006
emit(0xA9, 0xC0, 0x8D, 0x06, 0x20)  # LDA #$C0 / STA $2006
emit(0xA2, 0x40)                    # LDX #$40
label("attr_loop")
emit(0xA9, 0x00)                    # LDA #$00
emit(0x8D, 0x07, 0x20)              # STA $2007
emit(0xCA)                          # DEX
emit(0xD0, 0xF8)                    # BNE attr_loop

# scroll = 0
emit(0xA9, 0x00)                    # LDA #$00
emit(0x8D, 0x05, 0x20)              # STA $2005
emit(0x8D, 0x05, 0x20)              # STA $2005

# enable NMI + background
emit(0xA9, 0x80, 0x8D, 0x00, 0x20)  # LDA #$80 / STA $2000
emit(0xA9, 0x1E, 0x8D, 0x01, 0x20)  # LDA #$1E / STA $2001

label("main")
emit(0x4C, 0x00, 0x00)              # JMP main (patched below)

label("nmi")
emit(0xAD, 0x02, 0x20)              # LDA $2002
emit(0xE6, 0x10)                    # INC $10
emit(0xA9, 0x00)                    # LDA #$00
emit(0x8D, 0x05, 0x20)              # STA $2005  (fine X = 0)
emit(0xA5, 0x10)                    # LDA $10
emit(0x8D, 0x05, 0x20)              # STA $2005  (scroll Y)
emit(0x40)                          # RTI

label("pal_data")
PALETTE_BYTES = bytes([0x0F, 0x16, 0x2A, 0x12] * 8)
emit(*PALETTE_BYTES)

label("row_tiles")
emit(*([1] * 8 + [2] * 8 + [3] * 8 + [0] * 6))

# patch operands whose targets are now known
prg = bytearray(PRG_BANK)
prg[0:len(CODE)] = CODE
base = 0xC000


def patch_abs(index, address):
    prg[index] = address & 0xFF
    prg[index + 1] = (address >> 8) & 0xFF


# LDA pal_data,X at offset from label search
pal_loop_operand = prg.find(bytes([0xBD, 0x00, 0x00]))
patch_abs(pal_loop_operand + 1, labels["pal_data"])
row_operand = prg.find(bytes([0xB9, 0x00, 0x00]))
patch_abs(row_operand + 1, labels["row_tiles"])
jmp_operand = prg.find(bytes([0x4C, 0x00, 0x00]))
patch_abs(jmp_operand + 1, labels["main"])

# vectors (CPU $FFFA..$FFFF -> PRG offset 0x3FFA)
prg[0x3FFA:0x3FFC] = struct.pack("<H", labels["nmi"])
prg[0x3FFC:0x3FFE] = struct.pack("<H", labels["reset"])
prg[0x3FFE:0x4000] = struct.pack("<H", labels["reset"])

# --- CHR: four 8x8 tiles ------------------------------------------------
chr_data = bytearray(CHR_BANK)


def put_tile(index, plane0, plane1):
    offset = index * 16
    chr_data[offset:offset + 8] = bytes([plane0] * 8)
    chr_data[offset + 8:offset + 16] = bytes([plane1] * 8)


put_tile(0, 0xF0, 0x0F)  # checker: left color 1, right color 2
put_tile(1, 0xFF, 0x00)  # solid color 1
put_tile(2, 0x00, 0xFF)  # solid color 2
put_tile(3, 0xFF, 0xFF)  # solid color 3

header = bytearray(16)
header[0:4] = b"NES\x1A"
header[4] = 1  # 16 KiB PRG
header[5] = 1  # 8 KiB CHR
header[6] = 0  # mapper 0, vertical mirroring (horizontal arrangement)

output = pathlib.Path(__file__).resolve().parent.parent / "nes_band" / "assets" / "test_band.nes"
output.parent.mkdir(parents=True, exist_ok=True)
output.write_bytes(bytes(header) + bytes(prg) + bytes(chr_data))
print(f"wrote {output} ({output.stat().st_size} bytes)")
print("code bytes:", len(CODE), "labels:", {k: hex(v) for k, v in labels.items()})
