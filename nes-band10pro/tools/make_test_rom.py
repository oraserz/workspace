#!/usr/bin/env python3
"""Generate a minimal NROM (mapper 0) test ROM for the nespro display path.

The ROM is our own work (no copyrighted content): it draws a four-palette
checkerboard from CHR RAM so the host-side capture can be checked visually.
CHR RAM is used (chr_8kb = 0), so no CHR data is stored in the file.
"""

import struct
import sys
from pathlib import Path

BASE = 0x8000
DATA_OFF = 0x0400
PALDATA = BASE + DATA_OFF
CHRTILES = PALDATA + 32
ATTRTAB = CHRTILES + 64

code = bytearray()
fixups = []  # (pos, kind, target) kind: 'abs' | 'rel'


def emit(*bs):
    for b in bs:
        code.append(b)


def abs_x(op, addr):
    emit(op, addr & 0xFF, addr >> 8)


def rel(op, label):
    emit(op, 0)
    fixups.append((len(code) - 1, "rel", label))


def abs_branch(op, label):
    emit(op, 0, 0)
    fixups.append((len(code) - 2, "abs", label))


def label(name, at=None):
    fixups.append((len(code), "label", (name, at)))


# ---- data tables ----
paldata = bytes([
    0x0F, 0x16, 0x2A, 0x12,  # universal backdrop, red, green, blue
    0x0F, 0x27, 0x31, 0x30,  # black, orange, light blue, white
    0x0F, 0x19, 0x29, 0x39,  # black, greens
    0x0F, 0x01, 0x11, 0x21,  # black, dark blues
    0x0F, 0x16, 0x2A, 0x12,
    0x0F, 0x27, 0x31, 0x30,
    0x0F, 0x19, 0x29, 0x39,
    0x0F, 0x01, 0x11, 0x21,
])
assert len(paldata) == 32

# tile 0: 1px checker in colours 1/0, tiles 1..3 solid colours 1/2/3.
tile0 = bytes([0xAA, 0x55] * 4 + [0x00] * 8)
tile1 = bytes([0xFF] * 8 + [0x00] * 8)
tile2 = bytes([0x00] * 8 + [0xFF] * 8)
tile3 = bytes([0xFF] * 8 + [0xFF] * 8)
chrtiles = tile0 + tile1 + tile2 + tile3
assert len(chrtiles) == 64

attrtab = bytes([0xE4, 0x1B] * 32)
assert len(attrtab) == 64

# ---- program ----
emit(0x78)                    # SEI
emit(0xD8)                    # CLD
emit(0xA2, 0xFF)              # LDX #$FF
emit(0x9A)                    # TXS

label("wait1")
emit(0x2C, 0x02, 0x20)        # BIT $2002
rel(0x10, "wait1")            # BPL wait1
label("wait2")
emit(0x2C, 0x02, 0x20)        # BIT $2002
rel(0x10, "wait2")            # BPL wait2

emit(0xA9, 0x00)              # LDA #0
emit(0x8D, 0x00, 0x20)        # STA $2000
emit(0x8D, 0x01, 0x20)        # STA $2001

emit(0xA9, 0x3F)              # LDA #$3F
emit(0x8D, 0x06, 0x20)        # STA $2006
emit(0xA9, 0x00)
emit(0x8D, 0x06, 0x20)        # STA $2006
emit(0xA2, 0x00)              # LDX #0
label("palloop")
abs_x(0xBD, PALDATA)          # LDA paldata,X
emit(0x8D, 0x07, 0x20)        # STA $2007
emit(0xE8)                    # INX
emit(0xE0, 0x20)              # CPX #32
rel(0xD0, "palloop")          # BNE palloop

emit(0xA9, 0x00)              # LDA #0
emit(0x8D, 0x06, 0x20)        # STA $2006 -> $0000
emit(0x8D, 0x06, 0x20)
emit(0xA2, 0x00)
label("chrloop")
abs_x(0xBD, CHRTILES)         # LDA chrtiles,X
emit(0x8D, 0x07, 0x20)        # STA $2007
emit(0xE8)
emit(0xE0, 0x40)              # CPX #64
rel(0xD0, "chrloop")          # BNE chrloop

# nametable $2000: 960 tiles, all index 0 (checker tile)
emit(0xA9, 0x20)              # LDA #$20
emit(0x8D, 0x06, 0x20)
emit(0xA9, 0x00)
emit(0x8D, 0x06, 0x20)
emit(0xA2, 0x00)              # LDX #0
emit(0xA0, 0x03)              # LDY #3
label("nt_page")
label("nt_inner")
emit(0x8A)                    # TXA
emit(0x29, 0x03)              # AND #3 (cycle tile 0..3 across the screen)
emit(0x8D, 0x07, 0x20)        # STA $2007
emit(0xCA)                    # DEX
rel(0xD0, "nt_inner")
emit(0x88)                    # DEY
rel(0xD0, "nt_page")
emit(0xA2, 0xC0)              # LDX #192
label("nt_tail")
emit(0x8A)                    # TXA
emit(0x29, 0x03)
emit(0x8D, 0x07, 0x20)
emit(0xCA)
rel(0xD0, "nt_tail")

# attribute table $23C0: 64 alternating bytes
emit(0xA9, 0x23)
emit(0x8D, 0x06, 0x20)
emit(0xA9, 0xC0)
emit(0x8D, 0x06, 0x20)
emit(0xA2, 0x00)
label("attrloop")
abs_x(0xBD, ATTRTAB)
emit(0x8D, 0x07, 0x20)
emit(0xE8)
emit(0xE0, 0x40)
rel(0xD0, "attrloop")

# Reset the scroll/address latch to nametable 0 origin before turning on
# rendering; otherwise the PPU starts at the last VRAM write address.
emit(0xA9, 0x20)              # LDA #$20
emit(0x8D, 0x06, 0x20)
emit(0xA9, 0x00)
emit(0x8D, 0x06, 0x20)

emit(0xA9, 0x00)
emit(0x8D, 0x00, 0x20)        # PPUCTRL = 0 (NMI off, nametable 0)
emit(0xA9, 0x0A)
emit(0x8D, 0x01, 0x20)        # PPUMASK = show bg
label("forever")
abs_branch(0x4C, "forever")   # JMP forever

emit(0x40)                    # RTI (unused NMI vector)

# ---- layout into PRG ----
# 16 KiB NROM-128 image: it is mirrored at $8000-$BFFF and $C000-$FFFF, so the
# $8000-based code above lands at PRG offset 0 either way. The smaller image
# keeps the 128 KiB module heap free (see docs/DECISION.md memory accounting).
prg = bytearray(0x4000)
prg[0:len(code)] = code
prg[DATA_OFF:DATA_OFF + 32] = paldata
prg[DATA_OFF + 32:DATA_OFF + 96] = chrtiles
prg[DATA_OFF + 96:DATA_OFF + 160] = attrtab

# resolve fixups: label addresses are absolute ($8000-based)
labels = {}
for pos, kind, extra in fixups:
    if kind == "label":
        name, at = extra
        labels[name] = at if at is not None else BASE + pos

for pos, kind, target in fixups:
    if kind == "rel":
        addr_after = BASE + pos + 1
        off = labels[target] - addr_after
        assert -128 <= off <= 127, (target, off)
        prg[pos] = off & 0xFF
    elif kind == "abs":
        prg[pos] = labels[target] & 0xFF
        prg[pos + 1] = (labels[target] >> 8) & 0xFF

# vectors: NMI, RESET, IRQ. $8000 is the mirror base of PRG offset 0.
prg[0x3FFA:0x4000] = struct.pack("<HHH", BASE, BASE, BASE)

header = bytes([0x4E, 0x45, 0x53, 0x1A, 1, 0, 0x00, 0x00]) + bytes(8)
out = header + bytes(prg)

dest = Path(sys.argv[1] if len(sys.argv) > 1 else "roms/nespro-test.nes")
dest.parent.mkdir(parents=True, exist_ok=True)
dest.write_bytes(out)
print(f"wrote {dest} ({len(out)} bytes)")
