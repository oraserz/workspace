//! Emulator ownership, ROM source and one-frame stepping.
//!
//! The picture is produced by the same `x-nes` build the host exporter uses:
//! `Ppu::frame` holds 256x240 palette indices, and `video::convert_nes_frame`
//! turns them into RGB565 exactly as `host/` does when it writes a PNG.

use core::alloc::Layout;
use core::slice;

use nes::mapper::Mapper;
use nes::Emulator;

use crate::video::{convert_nes_frame, NES_FRAME_LEN};

/// Self-authored NROM test ROM (`tools/make_test_rom.py`); no copyrighted ROM
/// or firmware image is embedded.
const ROM: &[u8] = include_bytes!("../assets/test_band.nes");

/// A ROM that never reaches vblank must not stall the UI thread forever.
const MAX_INSTRUCTIONS_PER_FRAME: u32 = 4_000_000;

const RESULT_OK: i32 = 0;
const RESULT_NO_ROM: i32 = -1;
const RESULT_NO_MEMORY: i32 = -2;

struct Machine {
    emulator: *mut Emulator,
    pixels: *mut u16,
    frames: u32,
    instructions: u32,
}

static mut MACHINE: Machine = Machine {
    emulator: core::ptr::null_mut(),
    pixels: core::ptr::null_mut(),
    frames: 0,
    instructions: 0,
};

/// Allocates the emulator and the RGB565 picture buffer. Called from
/// `canopus_mod_activate`, never from `canopus_mod_prepare`.
pub fn init() -> i32 {
    let mapper = match mapper_from_rom(ROM) {
        Some(mapper) => mapper,
        None => return RESULT_NO_ROM,
    };

    let emulator = unsafe { alloc::alloc::alloc(Layout::new::<Emulator>()) }.cast::<Emulator>();
    if emulator.is_null() {
        return RESULT_NO_MEMORY;
    }
    let pixels = unsafe { alloc::alloc::alloc(Layout::array::<u16>(NES_FRAME_LEN).unwrap()) }
        .cast::<u16>();
    if pixels.is_null() {
        unsafe { alloc::alloc::dealloc(emulator.cast(), Layout::new::<Emulator>()) };
        return RESULT_NO_MEMORY;
    }

    // SAFETY: both allocations are fresh and uninitialized, and
    // `init_in_place` writes every field without building a stack temporary.
    unsafe {
        Emulator::init_in_place(emulator, mapper);
        core::ptr::addr_of_mut!(MACHINE).write(Machine {
            emulator,
            pixels,
            frames: 0,
            instructions: 0,
        });
    }
    RESULT_OK
}

/// Parses the iNES header and hands the bank slices to the mapper factory.
fn mapper_from_rom(data: &[u8]) -> Option<Mapper> {
    if data.len() < 16 || data[0..4] != [0x4E, 0x45, 0x53, 0x1A] {
        return None;
    }
    let flags6 = data[6];
    let flags7 = data[7];
    let mapper_id = ((flags7 & 0x0F) << 4) | (flags6 >> 4);
    let prg_banks = usize::from(data[4]);
    let chr_banks = usize::from(data[5]);
    let prg_size = prg_banks * 0x4000;
    let chr_size = chr_banks * 0x2000;
    let chr_ram = chr_banks == 0;
    let start = 16;
    if data.len() < start + prg_size + chr_size {
        return None;
    }
    let prg = &data[start..start + prg_size];
    let chr = &data[start + prg_size..start + prg_size + chr_size];
    Some(Mapper::from_ines(
        mapper_id,
        flags6 & 0x01,
        prg,
        chr,
        chr_ram,
    ))
}

/// Runs the emulator until the PPU completes one frame and refreshes the
/// RGB565 buffer. Returns `false` when the frame budget was exhausted.
pub fn run_frame() -> bool {
    // SAFETY: `init` populated both pointers before any page can trigger a
    // frame, and only the UI thread calls this.
    unsafe {
        let machine = &mut *core::ptr::addr_of_mut!(MACHINE);
        if machine.emulator.is_null() {
            return false;
        }
        let emulator = &mut *machine.emulator;
        emulator.bus.ppu.frame_complete = false;
        let mut instructions = 0u32;
        while !emulator.bus.ppu.frame_complete {
            let _ = emulator.tick();
            instructions += 1;
            if instructions >= MAX_INSTRUCTIONS_PER_FRAME {
                machine.instructions = machine.instructions.wrapping_add(instructions);
                return false;
            }
        }
        let out = slice::from_raw_parts_mut(machine.pixels, NES_FRAME_LEN);
        convert_nes_frame(&emulator.bus.ppu.frame, out);
        machine.frames = machine.frames.wrapping_add(1);
        machine.instructions = machine.instructions.wrapping_add(instructions);
        true
    }
}

/// Fixed RGB565 image buffer handed to LVGL as the image source.
pub fn pixels() -> *const u8 {
    // SAFETY: written by `init` before the page exists.
    unsafe { (*core::ptr::addr_of!(MACHINE)).pixels.cast::<u8>() }
}

pub fn is_ready() -> bool {
    // SAFETY: plain pointer read of module-owned static state.
    unsafe { !(*core::ptr::addr_of!(MACHINE)).emulator.is_null() }
}

/// `canopus_mod_query` tail: frames rendered and CPU instructions executed.
pub fn query_status() -> [u32; 2] {
    // SAFETY: plain reads of module-owned static state.
    unsafe {
        let machine = &*core::ptr::addr_of!(MACHINE);
        [machine.frames, machine.instructions]
    }
}
