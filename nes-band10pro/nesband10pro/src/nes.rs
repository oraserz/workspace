//! Emulator ownership and the per-tick frame pump.
//!
//! The `Emulator` value is ~76 KiB, so it is built in place on the module heap
//! (`Emulator::init_in`) instead of being returned by value. Load order keeps
//! the peak below the 128 KiB module budget: file bytes -> `Rom` -> mapper ->
//! drop both -> emulator. The RGB565 output is written back into the PPU frame
//! buffer, so no second pixel allocation is needed. See docs/02-implementation.md.

use alloc::alloc::alloc;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

use canopus_abi::*;
use canopus_runtime::fs::{data_path, File};
use nes::rom::Rom;
use nes::Emulator;

use crate::frame;

/// 20 fps UI tick; a frame needs ~29,780 CPU cycles.
pub const PERIOD_MS: u32 = 50;
/// Per-tick emulation budget: two frames' worth, so a slow tick still yields
/// one completed frame without stalling the UI thread indefinitely.
const BUDGET_CYCLES: u32 = 60_000;
/// Display size: 4:3, exactly one quarter of the NES frame per axis.
pub const DISPLAY_WIDTH: u32 = 128;
pub const DISPLAY_HEIGHT: u32 = 96;

static EMULATOR: AtomicPtr<Emulator> = AtomicPtr::new(core::ptr::null_mut());
/// RGB565 output. It aliases the PPU frame buffer (see `frame::convert_raw`),
/// so no second 24 KiB allocation is needed.
static PIXELS: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static WIDTH: AtomicU32 = AtomicU32::new(0);
static HEIGHT: AtomicU32 = AtomicU32::new(0);
static READY: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicI32 = AtomicI32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static LAST_FRAME_MS: AtomicU32 = AtomicU32::new(0);

const E_NO_ROM: i32 = -2;
const E_BAD_ROM: i32 = -22;
const E_NO_MEM: i32 = -12;

const ROM_NAMES: [&str; 3] = ["game.nes", "pattern.nes", "rom.nes"];
const ROM_DIRS: [&str; 2] = ["/data/canopus/nesband10pro", "/data/canopus"];

/// Loads the ROM and builds the emulator. Runs from `activate` (the stock
/// `insmod` 7.9 KiB stack only runs `prepare`, which must not allocate).
pub fn init(ctx: *const ContextV1) -> i32 {
    if READY.load(Ordering::Acquire) {
        return 0;
    }
    let Some(c) = crate::context(ctx) else {
        return E_BAD_ROM;
    };
    let bytes = match find_rom(ctx, c) {
        Some(v) => v,
        None => return fail(E_NO_ROM),
    };
    let rom = match Rom::new(&bytes) {
        Ok(rom) => rom,
        Err(_) => return fail(E_BAD_ROM),
    };
    drop(bytes);
    if rom.mapper_id != 0 {
        // Only mapper 0 (NROM) is promised by this round; recording the id
        // keeps a real cartridge from silently misrendering.
        let id = rom.mapper_id;
        drop(rom);
        return fail(-1000 - i32::from(id));
    }

    let mapper = rom.create_mapper();
    drop(rom);

    let emulator_size = core::mem::size_of::<Emulator>();
    if mem_available(ctx, c) < emulator_size as u32 + 4096 {
        return fail(E_NO_MEM);
    }
    let layout = Layout::from_size_align(emulator_size, core::mem::align_of::<Emulator>())
        .unwrap_or(Layout::new::<u8>());
    // SAFETY: the layout is non-zero and the module allocator returns at least
    // 8-byte alignment (documented limit in docs/02-implementation.md).
    let emulator = unsafe { alloc(layout) }.cast::<Emulator>();
    if emulator.is_null() {
        return fail(E_NO_MEM);
    }
    // SAFETY: `emulator` is a fresh, aligned, uninitialized Emulator allocation;
    // `init_in` writes every byte exactly once.
    unsafe { Emulator::init_in(emulator, mapper) };

    // The RGB565 frame is converted into the PPU frame buffer itself, one
    // aligned byte past its start (`u16` output needs 2-byte alignment).
    // `Ppu::frame` is 256*240 bytes, far more than 128*96*2.
    // SAFETY: `emulator` is a live, initialised Emulator; `addr_of_mut!` only
    // forms the address of the field without reading it.
    let frame_ptr = unsafe { core::ptr::addr_of_mut!((*emulator).bus.ppu.frame).cast::<u8>() };
    let pad = (frame_ptr as usize) & 1;
    let pixels = unsafe { frame_ptr.add(pad) };
    let pixel_bytes = (DISPLAY_WIDTH * DISPLAY_HEIGHT * 2) as usize;
    if 256 * 240 - pad < pixel_bytes {
        return fail(E_NO_MEM);
    }

    EMULATOR.store(emulator, Ordering::Release);
    PIXELS.store(pixels, Ordering::Release);
    WIDTH.store(DISPLAY_WIDTH, Ordering::Release);
    HEIGHT.store(DISPLAY_HEIGHT, Ordering::Release);
    STATUS.store(0, Ordering::Release);
    READY.store(true, Ordering::Release);
    0
}

fn fail(code: i32) -> i32 {
    STATUS.store(code, Ordering::Release);
    code
}

fn mem_available(ctx: *const ContextV1, c: &CanopusContext) -> u32 {
    if c.mem.is_null() {
        return 0;
    }
    unsafe { ((*c.mem).available)(ctx) }
}

fn find_rom(ctx: *const ContextV1, c: &CanopusContext) -> Option<Vec<u8>> {
    if c.fs.is_null() {
        return None;
    }
    let mut dir_buf = [0u8; 96];
    if let Ok(dir) = data_path(ctx.cast(), &mut dir_buf) {
        for name in ROM_NAMES {
            if let Some(bytes) = open_joined(ctx, dir, name) {
                return Some(bytes);
            }
        }
    }
    for dir in ROM_DIRS {
        for name in ROM_NAMES {
            if let Some(bytes) = open_joined(ctx, dir, name) {
                return Some(bytes);
            }
        }
    }
    None
}

fn open_joined(ctx: *const ContextV1, dir: &str, name: &str) -> Option<Vec<u8>> {
    let mut path = [0u8; 128];
    let needed = dir.len() + 1 + name.len();
    if needed > path.len() {
        return None;
    }
    path[..dir.len()].copy_from_slice(dir.as_bytes());
    path[dir.len()] = b'/';
    path[dir.len() + 1..needed].copy_from_slice(name.as_bytes());
    let path = core::str::from_utf8(&path[..needed]).ok()?;
    read_file(ctx, path)
}

fn read_file(ctx: *const ContextV1, path: &str) -> Option<Vec<u8>> {
    let mut file = File::open(ctx.cast(), path, FS_READ).ok()?;
    let size = file.size().ok()?;
    // NROM tops out at 32 KiB PRG + 8 KiB CHR, so anything above 64 KiB is
    // rejected before allocating: a larger `Vec` cannot fit the 128 KiB module
    // heap and would abort instead of reporting an error.
    if size <= 0 || size > 64 * 1024 {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(size as usize);
    let mut chunk = [0u8; 1024];
    let mut left = size as usize;
    while left > 0 {
        let want = left.min(chunk.len());
        let got = file.read(&mut chunk[..want]).ok()?;
        if got == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..got]);
        left -= got;
    }
    if out.is_empty() { None } else { Some(out) }
}

pub fn ready() -> bool {
    READY.load(Ordering::Acquire) && !PIXELS.load(Ordering::Acquire).is_null()
}

pub fn status() -> i32 {
    STATUS.load(Ordering::Acquire)
}

pub fn frames() -> u32 {
    FRAMES.load(Ordering::Acquire)
}

pub fn last_frame_ms() -> u32 {
    LAST_FRAME_MS.load(Ordering::Acquire)
}

pub fn dimensions() -> (u32, u32) {
    (
        WIDTH.load(Ordering::Acquire),
        HEIGHT.load(Ordering::Acquire),
    )
}

/// Runs the CPU until a frame completes or `BUDGET_CYCLES` is spent.
/// Returns `true` when a fresh frame is ready to convert.
pub fn step() -> bool {
    let emulator = EMULATOR.load(Ordering::Acquire);
    if emulator.is_null() {
        return false;
    }
    // SAFETY: only the UI thread enters this function, and the pointer stays
    // valid for the module's lifetime.
    let emulator = unsafe { &mut *emulator };
    emulator.bus.ppu.frame_complete = false;
    let mut spent = 0u32;
    while spent < BUDGET_CYCLES {
        spent += u32::from(emulator.tick());
        if emulator.bus.ppu.frame_complete {
            return true;
        }
    }
    false
}

/// Converts the current PPU frame into RGB565, in place inside the frame
/// buffer. Must run right after `step()` returned `true` and before the next
/// `step()`, while no LVGL draw is in progress.
pub fn convert() {
    let emulator = EMULATOR.load(Ordering::Acquire);
    let pixels = PIXELS.load(Ordering::Acquire);
    let width = WIDTH.load(Ordering::Acquire) as usize;
    let height = HEIGHT.load(Ordering::Acquire) as usize;
    if emulator.is_null() || pixels.is_null() || width == 0 || height == 0 {
        return;
    }
    // SAFETY: the source is the emulator's PPU frame (256x240 bytes) and the
    // destination is the same buffer (one aligned byte in), which fits
    // 128*96*2 bytes. `convert_raw` reads ahead of the write cursor, so the
    // overlap is safe; the next `step()` re-renders every pixel.
    unsafe {
        frame::convert_raw(
            (&(*emulator).bus.ppu.frame).as_ptr(),
            pixels.cast::<u16>(),
            width,
            height,
        );
    }
}

/// Non-null RGB565 buffer for the LVGL descriptor, or null before `init`.
pub fn pixel_buffer() -> *const u8 {
    PIXELS.load(Ordering::Acquire)
}

pub fn note_frame(elapsed_ms: u32) {
    FRAMES.store(FRAMES.load(Ordering::Relaxed).wrapping_add(1), Ordering::Release);
    LAST_FRAME_MS.store(elapsed_ms, Ordering::Release);
}

pub fn now_ms() -> u32 {
    crate::now_ms()
}
