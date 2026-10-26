#!/usr/bin/env python3
"""Generate a self-authored minimal NROM test ROM (no copyrighted content).

The ROM displays a 3-colour checkerboard of 32x32-pixel blocks built from four
solid CHR tiles. It is the phase-2 fixture for the nexband10pro module and for
the host-side frame check.

Layout:
  * iNES 1.0 header, mapper 0 (NROM), 16 KiB PRG, 8 KiB CHR, horizontal mirroring
  * PRG: 6502 init at $C000 that uploads the palette and one nametable, then
    enables background rendering and loops forever
  * CHR: tile 0 = colour 0, tile 1 = colour 1, tile 2 = colour 2, tile 3 = colour 3

Outputs:
  * assets/pattern.nes            (embedded by the module via include_bytes!)
  * build/testroms/pattern.nes    (host-side test fixture copy)
"""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
PRG_BASE = 0xC000
PRG_SIZE = 0x4000
CHR_SIZE = 0x2000


class Asm:
    def __init__(self, origin: int) -> None:
        self.origin = origin
        self.buf = bytearray()
        self.labels: dict[str, int] = {}
        self.fix: list[tuple[str, int, str]] = []

    @property
    def pc(self) -> int:
        return self.origin + len(self.buf)

    def label(self, name: str) -> None:
        self.labels[name] = self.pc

    def emit(self, *values: int) -> None:
        self.buf.extend(values)

    def abs_label(self, name: str) -> None:
        self.fix.append(("abs", len(self.buf), name))
        self.emit(0, 0)

    def imm_low(self, name: str) -> None:
        self.fix.append(("low", len(self.buf), name))
        self.emit(0)

    def imm_high(self, name: str) -> None:
        self.fix.append(("high", len(self.buf), name))
        self.emit(0)

    def rel(self, opcode: int, name: str) -> None:
        self.fix.append(("rel", len(self.buf), name))
        self.emit(opcode, 0)

    def resolve(self) -> bytearray:
        for kind, at, name in self.fix:
            target = self.labels[name]
            if kind == "abs":
                self.buf[at] = target & 0xFF
                self.buf[at + 1] = (target >> 8) & 0xFF
            elif kind == "low":
                self.buf[at] = target & 0xFF
            elif kind == "high":
                self.buf[at] = (target >> 8) & 0xFF
            else:
                delta = target - (self.origin + at + 2)
                if not -128 <= delta <= 127:
                    raise ValueError(f"branch to {name} out of range: {delta}")
                self.buf[at + 1] = delta & 0xFF
        return self.buf


def build_prg() -> bytes:
    a = Asm(PRG_BASE)
    a.emit(0x78)                 # SEI
    a.emit(0xA2, 0xFF)           # LDX #$FF
    a.emit(0x9A)                 # TXS
    a.label("vblank1")
    a.emit(0x2C, 0x02, 0x20)     # BIT $2002
    a.rel(0x10, "vblank1")       # BPL vblank1
    a.label("vblank2")
    a.emit(0x2C, 0x02, 0x20)     # BIT $2002
    a.rel(0x10, "vblank2")       # BPL vblank2

    a.emit(0xA9, 0x00)           # LDA #$00
    a.emit(0x8D, 0x00, 0x20)     # STA $2000 (NMI off, BG pattern table 0)
    a.emit(0x8D, 0x01, 0x20)     # STA $2001 (rendering off)

    a.emit(0xA9, 0x3F)           # LDA #$3F
    a.emit(0x8D, 0x06, 0x20)     # STA $2006
    a.emit(0xA9, 0x00)           # LDA #$00
    a.emit(0x8D, 0x06, 0x20)     # STA $2006
    a.emit(0xA2, 0x00)           # LDX #$00
    a.label("pal_loop")
    a.emit(0xBD)                 # LDA pal_data,X
    a.abs_label("pal_data")
    a.emit(0x8D, 0x07, 0x20)     # STA $2007
    a.emit(0xE8)                 # INX
    a.emit(0xE0, 0x20)           # CPX #$20
    a.rel(0xD0, "pal_loop")      # BNE pal_loop

    a.emit(0xA9, 0x20)           # LDA #$20
    a.emit(0x8D, 0x06, 0x20)     # STA $2006
    a.emit(0xA9, 0x00)           # LDA #$00
    a.emit(0x8D, 0x06, 0x20)     # STA $2006

    a.emit(0xA9)                 # LDA #<nt_data
    a.imm_low("nt_data")
    a.emit(0x85, 0x00)           # STA $00
    a.emit(0xA9)                 # LDA #>nt_data
    a.imm_high("nt_data")
    a.emit(0x85, 0x01)           # STA $01
    a.emit(0xA9, 0xC0)           # LDA #$C0
    a.emit(0x85, 0x02)           # STA $02
    a.emit(0xA9, 0x03)           # LDA #$03
    a.emit(0x85, 0x03)           # STA $03
    a.emit(0xA0, 0x00)           # LDY #$00

    a.label("copy_check")
    a.emit(0xA5, 0x02)           # LDA $02
    a.emit(0x05, 0x03)           # ORA $03
    a.rel(0xF0, "done")          # BEQ done
    a.emit(0xB1, 0x00)           # LDA ($00),Y
    a.emit(0x8D, 0x07, 0x20)     # STA $2007
    a.emit(0xE6, 0x00)           # INC $00
    a.rel(0xD0, "no_high_inc")   # BNE no_high_inc
    a.emit(0xE6, 0x01)           # INC $01
    a.label("no_high_inc")
    a.emit(0xA5, 0x02)           # LDA $02
    a.rel(0xD0, "dec_low")       # BNE dec_low
    a.emit(0xC6, 0x03)           # DEC $03
    a.label("dec_low")
    a.emit(0xC6, 0x02)           # DEC $02
    a.emit(0x4C)                 # JMP copy_check
    a.abs_label("copy_check")

    a.label("done")
    # Restore v = t = $2000 (fine Y and coarse X/Y = 0). The $2007 writes above
    # left the internal address at $23C0 (fine Y = 2); real hardware reloads v
    # from t at the pre-render line, but a ROM should not depend on that.
    a.emit(0xA9, 0x20)           # LDA #$20
    a.emit(0x8D, 0x06, 0x20)     # STA $2006
    a.emit(0xA9, 0x00)           # LDA #$00
    a.emit(0x8D, 0x06, 0x20)     # STA $2006
    a.emit(0x8D, 0x05, 0x20)     # STA $2005
    a.emit(0x8D, 0x05, 0x20)     # STA $2005
    a.emit(0xA9, 0x08)           # LDA #$08
    a.emit(0x8D, 0x00, 0x20)     # STA $2000
    a.emit(0xA9, 0x0A)           # LDA #$0A
    a.emit(0x8D, 0x01, 0x20)     # STA $2001 (show background)
    a.label("forever")
    a.emit(0x4C)                 # JMP forever
    a.abs_label("forever")

    a.label("pal_data")
    palette = bytes([0x0F, 0x16, 0x2A, 0x12] + [0x0F] * 28)
    a.buf.extend(palette)

    a.label("nt_data")
    nametable = bytearray()
    for row in range(30):
        for col in range(32):
            nametable.append(1 + ((col // 4 + row // 4) % 3))
    nametable.extend(bytes(64))  # attribute table: all palette 0
    a.buf.extend(nametable)

    prg = bytearray(PRG_SIZE)
    code = a.resolve()
    if len(code) > PRG_SIZE:
        raise ValueError("PRG overflow")
    prg[: len(code)] = code
    # Vectors live at the end of the 16 KiB bank: NMI, RESET, IRQ.
    for off in (0x3FFA, 0x3FFC, 0x3FFE):
        prg[off] = 0x00
        prg[off + 1] = 0xC0
    return bytes(prg)


def build_chr() -> bytes:
    chr_data = bytearray(CHR_SIZE)
    tiles = [
        bytes(16),                                             # colour 0
        bytes([0xFF] * 8 + [0x00] * 8),                        # colour 1
        bytes([0x00] * 8 + [0xFF] * 8),                        # colour 2
        bytes([0xFF] * 16),                                    # colour 3
    ]
    for index, tile in enumerate(tiles):
        chr_data[index * 16 : index * 16 + 16] = tile
    return bytes(chr_data)


def main() -> int:
    header = bytearray(b"NES\x1a")
    header.extend([1, 1, 0x00, 0x00])  # 16 KiB PRG, 8 KiB CHR, mapper 0
    header.extend(bytes(8))
    rom = bytes(header) + build_prg() + build_chr()

    outputs = [ROOT / "assets" / "pattern.nes", ROOT / "build" / "testroms" / "pattern.nes"]
    for out in outputs:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(rom)
        print(f"wrote {out} ({len(rom)} bytes)")

    if len(rom) != 16 + PRG_SIZE + CHR_SIZE:
        print("unexpected ROM size", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
