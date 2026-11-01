//! Minimal NROM/UxROM/CNROM cartridge mappers for the on-watch build.
//!
//! x-nes keeps each mapper's PRG/CHR in inline arrays and returns it by value:
//! `x_nes::mapper::nintendo::nrom::Nrom::new` alone needs a 40 KiB stack frame
//! and `Mapper::from_ines` a 48 KiB one (both measured in the built ELF). Page
//! callbacks run on the firmware UI task, whose stack is only a few KiB, so the
//! module supplies the same three mapper behaviours backed by heap vectors.
//! Anything else falls back to the embedded test ROM.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use nes::mapper::{Mapper, MapperImpl};

const PRG_BANK: usize = 0x4000;
const CHR_BANK: usize = 0x2000;
const PRG_RAM: usize = 0x2000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Nrom,
    UxRom,
    CnRom,
}

pub struct Cart {
    prg: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_ram: bool,
    mirror: u8,
    kind: Kind,
    prg_bank: u8,
    chr_bank: u8,
}

impl Cart {
    /// Builds a mapper for the parsed iNES ROM. `None` for unsupported mappers.
    pub fn new(
        mut prg: Vec<u8>,
        mut chr: Vec<u8>,
        chr_ram: bool,
        mirror: u8,
        mapper_id: u8,
    ) -> Option<Self> {
        let kind = match mapper_id {
            0 => Kind::Nrom,
            2 => Kind::UxRom,
            3 => Kind::CnRom,
            _ => return None,
        };
        if prg.is_empty() {
            return None;
        }
        // Pad to whole banks so every index below is in range.
        let prg_len = prg.len().div_ceil(PRG_BANK).max(1) * PRG_BANK;
        prg.resize(prg_len, 0);
        let chr_len = chr.len().div_ceil(CHR_BANK).max(1) * CHR_BANK;
        chr.resize(chr_len, 0);
        Some(Self {
            prg,
            chr,
            prg_ram: vec![0u8; PRG_RAM],
            chr_ram,
            mirror,
            kind,
            prg_bank: 0,
            chr_bank: 0,
        })
    }

    pub fn into_mapper(self) -> Mapper {
        Mapper(Box::new(self))
    }

    fn prg_banks(&self) -> usize {
        (self.prg.len() / PRG_BANK).max(1)
    }

    fn prg_byte(&self, addr: u16) -> u8 {
        let offset = (addr & 0x3FFF) as usize;
        let bank = match self.kind {
            Kind::UxRom => {
                if addr < 0xC000 {
                    (self.prg_bank as usize) % self.prg_banks()
                } else {
                    self.prg_banks() - 1
                }
            }
            // NROM/CNROM: 16 KiB PRG is mirrored across both halves.
            Kind::Nrom | Kind::CnRom => {
                if self.prg_banks() == 1 {
                    0
                } else {
                    ((addr as usize) >> 14) & (self.prg_banks() - 1)
                }
            }
        };
        self.prg[bank * PRG_BANK + offset]
    }

    fn chr_banks(&self) -> usize {
        (self.chr.len() / CHR_BANK).max(1)
    }
}

impl MapperImpl for Cart {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr & 0x1FFF) as usize],
            0x8000..=0xFFFF => self.prg_byte(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr & 0x1FFF) as usize] = val,
            0x8000..=0xFFFF => match self.kind {
                Kind::UxRom => self.prg_bank = val,
                Kind::CnRom => self.chr_bank = val,
                Kind::Nrom => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr >= CHR_BANK as u16 {
            return 0;
        }
        let bank = (self.chr_bank as usize) % self.chr_banks();
        self.chr[bank * CHR_BANK + addr as usize]
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if self.chr_ram {
            let bank = (self.chr_bank as usize) % self.chr_banks();
            self.chr[bank * CHR_BANK + (addr & 0x1FFF) as usize] = val;
        }
    }

    fn mirroring(&self) -> u8 {
        self.mirror
    }

    fn irq_pending(&self) -> bool {
        false
    }

    fn ack_irq(&mut self) {}

    fn has_chr_ram(&self) -> bool {
        self.chr_ram
    }
}
