//! nespro — NES emulator display module for Xiaomi Band 10 Pro (043).
//!
//! The module registers a native launcher application. When its page is created
//! it builds the widgets and starts a Supervisor time-service timer. That timer
//! runs the `x-nes` core off the LVGL task; each completed frame is converted to
//! RGB565 and the `lv_image` source is swapped from a `ui.post` callback, so the
//! UI thread only ever does the blit and never the 6502 emulation.
//!
//! Evidence level: compiled and ELF-verified only. The firmware-private LVGL
//! entry points used here are STATIC_RECOVERED, not device-verified.

#![no_std]

extern crate alloc;

mod cart;
mod nes_frame;

use alloc::vec::Vec;
use canopus_abi::*;
use canopus_runtime::{mem, status_put_u32, status_writer_publish};
use canopus_target_private::{activity_navigate, lvx_content_create, lvx_image_create,
    lvx_image_set_scale, lvx_image_set_src, lvx_object_align, lvx_object_set_size,
    ALIGN_TOP_MID, CONTENT_HEIGHT, CONTENT_TOP_OFFSET, CONTENT_WIDTH};
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_void};
use core::mem::MaybeUninit;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU8, Ordering};
use nes::cpu::CpuRp2a03;
use nes::mapper::Mapper;
use nes::rom::Rom;
use nes::{reset, Emulator};
use nes_frame::{NES_HEIGHT, NES_PIXELS, NES_WIDTH, convert_frame};

const MAGIC: u32 = 0x4E455350; // "NESP"
/// The Supervisor time pump is an LVGL timer on the UI task, so every callback
/// steals UI time. Fire often and keep each slice small instead of doing a
/// whole frame at once.
const TIMER_PERIOD_MS: u32 = 10;
/// Wall-clock budget for one emulation slice (the rest of the tick stays with
/// the UI task).
const SLICE_TARGET_MS: u32 = 4;
/// Adaptive instruction budget per slice, in instructions.
const BUDGET_START: u32 = 1024;
const BUDGET_MIN: u32 = 256;
const BUDGET_MAX: u32 = 4096;
/// Never republish faster than this; the panel cannot show more anyway.
const PUBLISH_MIN_MS: u32 = 50;

// ---------------------------------------------------------------------------
// Context + allocator
// ---------------------------------------------------------------------------

static CTX: AtomicPtr<ContextV1> = AtomicPtr::new(null_mut());

fn set_ctx(ctx: *const ContextV1) {
    CTX.store(ctx as *mut ContextV1, Ordering::Release);
}

fn ctx() -> *const ContextV1 {
    CTX.load(Ordering::Acquire) as *const ContextV1
}

/// Global allocator backed by the Canopus `mem` service (128 KiB module heap).
struct ModuleAlloc;

unsafe impl GlobalAlloc for ModuleAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let c = ctx();
        if c.is_null() {
            return null_mut();
        }
        let align = layout.align();
        if align <= 8 {
            return mem::alloc(c as *const CanopusContext, layout.size());
        }
        // Over-allocate and park the original pointer just before the aligned
        // block; dealloc reads it back. No SDK allocation needs this today.
        let Some(total) = layout.size().checked_add(align + 4) else {
            return null_mut();
        };
        let base = mem::alloc(c as *const CanopusContext, total);
        if base.is_null() {
            return null_mut();
        }
        let aligned = ((base as usize + 4 + align - 1) & !(align - 1)) as *mut u8;
        (aligned as *mut u32).write(base as usize as u32);
        aligned
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let c = ctx();
        if c.is_null() || ptr.is_null() {
            return;
        }
        let base = if layout.align() <= 8 {
            ptr
        } else {
            (ptr as *const u32).read() as usize as *mut u8
        };
        mem::free(c as *const CanopusContext, base);
    }
}

#[global_allocator]
static ALLOC: ModuleAlloc = ModuleAlloc;

// ---------------------------------------------------------------------------
// Page descriptor layout (firmware-private, 32-bit target)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct PageDescriptor {
    parent_descriptor: *mut c_void, // +0x00
    _pad_04: [u8; 0x0c],
    page_name: *mut c_void, // +0x10
    page_id: u16,           // +0x14
    app_id: u16,            // +0x16
    flags: u16,             // +0x18
    _pad_1a: [u8; 2],
    scheduler_deadline: i32,     // +0x1c
    scheduler_priority: i32,     // +0x20
    async_destroy_state: u32,    // +0x24
    lifecycle_state: u8,         // +0x28
    layer: u8,                   // +0x29
    page_kind: u8,               // +0x2a
    _pad_2b: [u8; 1],
    activity_context: *mut c_void, // +0x2c
    root_object: *mut c_void,      // +0x30
    on_signal: *mut c_void,        // +0x34
    runtime_default_56: *mut c_void, // +0x38
    _pad_3c: [u8; 4],
    registry_prev: *mut c_void, // +0x40
    registry_next: *mut c_void, // +0x44
    runtime_parent: *mut c_void, // +0x48
    on_create: *mut c_void,     // +0x4c
    on_resume: *mut c_void,     // +0x50
    _on_foreground_data: *mut c_void, // +0x54
    on_pause: *mut c_void,   // +0x58
    on_destroy: *mut c_void, // +0x5c
    on_ui_destroy: *mut c_void, // +0x60
    _ext_100: *mut c_void,   // +0x64
    _ext_104: *mut c_void,   // +0x68
    _ext_108: *mut c_void,   // +0x6c
    _tail: [u8; 4],
}

const _: () = assert!(core::mem::offset_of!(PageDescriptor, on_create) == 0x4c);
const _: () = assert!(core::mem::offset_of!(PageDescriptor, on_destroy) == 0x5c);
const _: () = assert!(core::mem::size_of::<PageDescriptor>() == 0x74);

const fn zero_page() -> PageDescriptor {
    PageDescriptor {
        parent_descriptor: null_mut(),
        _pad_04: [0; 0x0c],
        page_name: null_mut(),
        page_id: 0,
        app_id: 0,
        flags: 0,
        _pad_1a: [0; 2],
        scheduler_deadline: 0,
        scheduler_priority: 0,
        async_destroy_state: 0,
        lifecycle_state: 0,
        layer: 0,
        page_kind: 0,
        _pad_2b: [0; 1],
        activity_context: null_mut(),
        root_object: null_mut(),
        on_signal: null_mut(),
        runtime_default_56: null_mut(),
        _pad_3c: [0; 4],
        registry_prev: null_mut(),
        registry_next: null_mut(),
        runtime_parent: null_mut(),
        on_create: null_mut(),
        on_resume: null_mut(),
        _on_foreground_data: null_mut(),
        on_pause: null_mut(),
        on_destroy: null_mut(),
        on_ui_destroy: null_mut(),
        _ext_100: null_mut(),
        _ext_104: null_mut(),
        _ext_108: null_mut(),
        _tail: [0; 4],
    }
}

static mut PAGE: PageDescriptor = zero_page();
static mut PAGE_NAME: [u8; 9] = *b"nes_main\0";
static APP_ID: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// LVGL image descriptor + framebuffer
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct LvImageDsc {
    magic_cf_flags: u32,
    width_height: u32,
    stride_reserved: u32,
    data_size: u32,
    data: *const u16,
    reserved: *const c_void,
}

const DSC_ZERO: LvImageDsc = LvImageDsc {
    magic_cf_flags: 0,
    width_height: 0,
    stride_reserved: 0,
    data_size: 0,
    data: null(),
    reserved: null(),
};

static mut DSC: [LvImageDsc; 2] = [DSC_ZERO, DSC_ZERO];
static mut FB: [u16; NES_PIXELS] = [0; NES_PIXELS];
static DSC_INDEX: AtomicU8 = AtomicU8::new(0);
static IMAGE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// Supervisor time-service timer id (`-1` when none exists).
static RUN_TIMER: AtomicI32 = AtomicI32::new(-1);
/// `1` while the page is foreground; paused pages do not run the emulator.
static PAGE_ACTIVE: AtomicU8 = AtomicU8::new(0);
/// `1` when a converted frame waits for the UI task to swap the image source.
static FRAME_PENDING: AtomicU8 = AtomicU8::new(0);
/// Checksum of the last queued frame; identical frames skip conversion and
/// the LVGL redraw entirely.
static LAST_FRAME_HASH: AtomicU32 = AtomicU32::new(0);
/// `0` until the first frame has been queued (so a hash of zero still draws).
static HAS_FRAME: AtomicU8 = AtomicU8::new(0);

// Per-session performance counters, reported once when the page closes.
static BUDGET: AtomicU32 = AtomicU32::new(BUDGET_START);
static LAST_PUBLISH_MS: AtomicU32 = AtomicU32::new(0);
static SESSION_START_MS: AtomicU32 = AtomicU32::new(0);
static TICKS: AtomicU32 = AtomicU32::new(0);
static INSTRUCTIONS: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static PUBLISHED: AtomicU32 = AtomicU32::new(0);
static SLICE_SUM_MS: AtomicU32 = AtomicU32::new(0);
static SLICE_MAX_MS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Emulator state
// ---------------------------------------------------------------------------

static mut EMU: MaybeUninit<Emulator> = MaybeUninit::uninit();
static mut EMU_READY: bool = false;
static FIRST_FRAME_LOGGED: AtomicU8 = AtomicU8::new(0);

/// Embedded generated NROM test ROM (our own work, no copyrighted content).
static TEST_ROM: &[u8] = include_bytes!("../../roms/nespro-test.nes");

// ---------------------------------------------------------------------------
// Display parameters
// ---------------------------------------------------------------------------

/// Reads the panel resolution from the runtime instead of trusting a constant.
unsafe fn display_resolution(c: *const ContextV1) -> (u32, u32) {
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return (0, 0);
    };
    let Some(display) = (unsafe { ctx_ref.display.as_ref() }) else {
        return (0, 0);
    };
    let mut width = 0u32;
    let mut height = 0u32;
    unsafe { (display.get_resolution)(c, &mut width, &mut height) };
    (width, height)
}

unsafe fn log_info(c: *const ContextV1, message: &'static [u8]) {
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return;
    };
    let Some(log) = (unsafe { ctx_ref.log.as_ref() }) else {
        return;
    };
    unsafe { (log.info)(c, message.as_ptr() as *const c_char) };
}

// ---------------------------------------------------------------------------
// ROM loading
// ---------------------------------------------------------------------------

/// Names tried inside the module's private directory, in order. The FS service
/// resolves relative names to `/data/canopus/m.nespro/`; absolute paths under
/// `/data/canopus` are refused by the Supervisor.
const ROM_NAMES: [&[u8]; 3] = [b"nespro.nes\0", b"supermary.nes\0", b"rom.nes\0"];

unsafe fn read_rom_from_fs(c: *const ContextV1) -> Option<Vec<u8>> {
    if !unsafe { services_has(c, SVC_FS, 1) } {
        return None;
    }
    let ctx_ref = unsafe { (c as *const CanopusContext).as_ref() }?;
    let fs = unsafe { ctx_ref.fs.as_ref() }?;
    let mut stat = CanopusFsStat {
        size: 0,
        is_dir: 0,
        _pad: [0; 3],
        mtime: 0,
    };
    for name in ROM_NAMES {
        let path = name.as_ptr() as *const c_char;
        if unsafe { (fs.exists)(c, path) } <= 0 {
            continue;
        }
        if unsafe { (fs.stat)(c, path, &mut stat) } != 0 || stat.size == 0 {
            continue;
        }
        let mut buf: Vec<u8> = alloc::vec![0u8; stat.size as usize];
        let read =
            unsafe { (fs.read_all)(c, path, buf.as_mut_ptr() as *mut c_void, stat.size) };
        if read <= 0 {
            continue;
        }
        buf.truncate(read as usize);
        return Some(buf);
    }
    None
}

/// Initialises a 76 KiB `Emulator` in place instead of on the current stack.
///
/// `Emulator::new`, `Bus::new` and `Ppu::new` all return their whole state by
/// value, so calling them materialises the entire emulator (76,344 B) in the
/// caller's frame. Page callbacks run on the firmware UI task, whose stack is
/// only a few KiB: that overflow is what froze the watch as soon as the module
/// was enabled. Every write below goes through a raw pointer, so the largest
/// temporary the compiler can create is one `Mapper` (two words).
unsafe fn emulator_init_in_place(dst: *mut Emulator, mapper: Mapper) {
    let bus = unsafe { core::ptr::addr_of_mut!((*dst).bus) };
    let cpu = unsafe { core::ptr::addr_of_mut!((*dst).cpu) };
    // A zeroed image is a valid video-only state: `Ppu::new` only differs in
    // `odd_frame` (written below) and sprite-0-hit tracking, which is re-armed
    // at every prerender line; `Apu::new`'s other non-zero defaults only
    // change audio timbre, and its sample period is set below.
    unsafe { core::ptr::write_bytes(dst as *mut u8, 0, core::mem::size_of::<Emulator>()) };
    unsafe { core::ptr::addr_of_mut!((*bus).mapper).write(mapper) };
    unsafe { core::ptr::addr_of_mut!((*bus).ppu.odd_frame).write(true) };
    unsafe { (*bus).apu.set_sample_rate(44_100) };
    unsafe { cpu.write(CpuRp2a03::new(0)) };
    // Same final step as `Emulator::new`: load the reset vector through the bus.
    unsafe { reset(&mut *cpu, &mut *bus) };
}

unsafe fn start_emulator(rom_bytes: &[u8]) -> bool {
    let Ok(rom) = Rom::new(rom_bytes) else {
        return false;
    };
    let Rom {
        prg,
        chr,
        mapper_id,
        mirroring,
        has_chr_ram,
    } = rom;
    // Heap-backed mappers: x-nes builds them in 40 KiB inline arrays on the
    // stack (`Nrom::new`) or 48 KiB (`Mapper::from_ines`), which cannot fit in
    // the UI task stack. `Cart` only moves the parsed vectors.
    let Some(cart) = cart::Cart::new(prg, chr, has_chr_ram, mirroring, mapper_id) else {
        return false;
    };
    unsafe {
        emulator_init_in_place(EMU.as_mut_ptr(), cart.into_mapper());
        EMU_READY = true;
    }
    true
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn init_dsc() {
    let dsc = unsafe { &mut *core::ptr::addr_of_mut!(DSC) };
    for d in dsc.iter_mut() {
        d.magic_cf_flags = 0x19 | (0x12 << 8); // LV_IMAGE_HEADER_MAGIC | LV_COLOR_FORMAT_RGB565
        d.width_height = (NES_WIDTH as u32) | ((NES_HEIGHT as u32) << 16);
        d.stride_reserved = ((NES_WIDTH * 2) as u32) & 0xFFFF; // stride in bytes
        d.data_size = (NES_WIDTH * NES_HEIGHT * 2) as u32;
        d.data = unsafe { FB.as_ptr() };
        d.reserved = null();
    }
}

/// Checksum of the palette-index frame: unchanged screens never reach
/// `convert_frame` or the LVGL redraw.
unsafe fn frame_hash(frame: &[u8; NES_PIXELS]) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    for &byte in frame {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash
}

/// Loads the ROM from the module's private directory and boots the emulator.
/// Runs on the Supervisor timer task, never on the LVGL task.
unsafe fn ensure_emulator(c: *const ContextV1) -> bool {
    let rom = read_rom_from_fs(c);
    let started = match rom.as_deref() {
        Some(bytes) => {
            log_info(c, b"nespro rom nespro.nes\0");
            if start_emulator(bytes) {
                true
            } else {
                log_info(c, b"nespro rom unsupported, builtin test rom\0");
                start_emulator(TEST_ROM)
            }
        }
        None => {
            log_info(c, b"nespro rom builtin test rom\0");
            start_emulator(TEST_ROM)
        }
    };
    if !started {
        log_info(c, b"nespro start failed\0");
    }
    started
}

/// Wall clock from the Supervisor time service.
unsafe fn now_ms(c: *const ContextV1) -> u32 {
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return 0;
    };
    let Some(time) = (unsafe { ctx_ref.time.as_ref() }) else {
        return 0;
    };
    unsafe { (time.monotonic_ms)(c) }
}

/// Writes the session's measured throughput once, when the page closes. It is
/// a single small file, so flash wear is negligible compared with per-frame
/// logging.
unsafe fn write_perf(c: *const ContextV1) {
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return;
    };
    let ticks = TICKS.load(Ordering::Relaxed);
    let instructions = INSTRUCTIONS.load(Ordering::Relaxed);
    let frames = FRAMES.load(Ordering::Relaxed);
    let published = PUBLISHED.load(Ordering::Relaxed);
    let avg = if ticks > 0 { SLICE_SUM_MS.load(Ordering::Relaxed) / ticks } else { 0 };
    let max = SLICE_MAX_MS.load(Ordering::Relaxed);
    let session = now_ms(c).wrapping_sub(SESSION_START_MS.load(Ordering::Relaxed));
    let ips = if session > 0 {
        (instructions as u64) * 1000 / session as u64
    } else {
        0
    };
    let text = alloc::format!(
        "ticks={} instr={} frames={} published={} avg_slice_ms={} max_slice_ms={} session_ms={} instr_per_s={} budget={}\n",
        ticks, instructions, frames, published, avg, max, session, ips, BUDGET.load(Ordering::Relaxed)
    );
    let _ = canopus_runtime::fs::write_all(ctx_ref, "perf.txt", text.as_bytes());
}

/// UI timer-tick: emulate for at most one small slice, then hand the finished
/// frame to the UI task. The Supervisor pump runs on the LVGL task, so the
/// slice is capped by wall time and its instruction budget adapts to the
/// measured speed instead of blocking a whole frame at a time.
extern "C" fn runtime_tick(c: *const ContextV1, _timer_id: i32, _data: *mut c_void) {
    if PAGE_ACTIVE.load(Ordering::Acquire) == 0 {
        return;
    }
    unsafe {
        if !EMU_READY {
            if !ensure_emulator(c) {
                return;
            }
            EMU_READY = true;
            FRAME_PENDING.store(0, Ordering::Release);
        }
        if FRAME_PENDING.load(Ordering::Acquire) != 0 {
            return;
        }
        let start = now_ms(c);
        if SESSION_START_MS.load(Ordering::Acquire) == 0 {
            SESSION_START_MS.store(start.max(1), Ordering::Release);
        }
        TICKS.fetch_add(1, Ordering::Relaxed);

        let emu = &mut *EMU.as_mut_ptr();
        let budget = BUDGET.load(Ordering::Acquire);
        let mut executed = 0u32;
        let mut frame_done = false;
        let mut time_limited = false;
        while executed < budget {
            let _ = emu.tick();
            executed += 1;
            if emu.bus.ppu.frame_complete {
                emu.bus.ppu.frame_complete = false;
                frame_done = true;
                break;
            }
            if (executed & 31) == 0 {
                let elapsed = now_ms(c).wrapping_sub(start);
                if elapsed >= SLICE_TARGET_MS {
                    time_limited = true;
                    break;
                }
            }
        }
        INSTRUCTIONS.fetch_add(executed, Ordering::Relaxed);
        let slice_ms = now_ms(c).wrapping_sub(start);
        SLICE_SUM_MS.fetch_add(slice_ms, Ordering::Relaxed);
        if slice_ms > SLICE_MAX_MS.load(Ordering::Relaxed) {
            SLICE_MAX_MS.store(slice_ms, Ordering::Relaxed);
        }
        if time_limited && slice_ms > 0 {
            let next = (budget.saturating_mul(SLICE_TARGET_MS) / slice_ms)
                .clamp(BUDGET_MIN, BUDGET_MAX);
            BUDGET.store(next, Ordering::Release);
        } else if !frame_done && executed >= budget {
            BUDGET.store(budget.saturating_mul(2).min(BUDGET_MAX), Ordering::Release);
        }
        if !frame_done {
            return;
        }
        FRAMES.fetch_add(1, Ordering::Relaxed);

        let now = now_ms(c);
        let last = LAST_PUBLISH_MS.load(Ordering::Acquire);
        if last != 0 && now.wrapping_sub(last) < PUBLISH_MIN_MS {
            return;
        }
        let hash = frame_hash(&emu.bus.ppu.frame);
        if HAS_FRAME.load(Ordering::Acquire) != 0
            && hash == LAST_FRAME_HASH.load(Ordering::Acquire)
        {
            return;
        }
        LAST_FRAME_HASH.store(hash, Ordering::Release);
        HAS_FRAME.store(1, Ordering::Release);
        LAST_PUBLISH_MS.store(now.max(1), Ordering::Release);
        PUBLISHED.fetch_add(1, Ordering::Relaxed);
        FRAME_PENDING.store(1, Ordering::Release);
        let Some(ctx_ref) = ((c as *const CanopusContext).as_ref()) else {
            FRAME_PENDING.store(0, Ordering::Release);
            return;
        };
        let Some(ui) = ctx_ref.ui.as_ref() else {
            publish_frame(c, null_mut());
            return;
        };
        if (ui.post)(c, publish_frame, null_mut()) < 0 {
            publish_frame(c, null_mut());
        }
    }
}

/// UI-task half of a frame: convert the indices, then swap the image source.
extern "C" fn publish_frame(c: *const ContextV1, _data: *mut c_void) {
    unsafe {
        let image = IMAGE.load(Ordering::Acquire);
        if image.is_null() || !EMU_READY {
            FRAME_PENDING.store(0, Ordering::Release);
            return;
        }
        let emu = &mut *EMU.as_mut_ptr();
        convert_frame(&emu.bus.ppu.frame, &mut FB);
        let next = 1 - DSC_INDEX.load(Ordering::Relaxed);
        DSC_INDEX.store(next, Ordering::Relaxed);
        let dsc = &DSC[next as usize] as *const LvImageDsc as *const c_void;
        lvx_image_set_src(image, dsc);
        FRAME_PENDING.store(0, Ordering::Release);
        if FIRST_FRAME_LOGGED.swap(1, Ordering::Relaxed) == 0 {
            log_info(c, b"nespro first frame\0");
        }
    }
}

/// Creates (first call) and starts the Supervisor timer-service pump.
unsafe fn start_runtime(c: *const ContextV1) {
    if !unsafe { services_has(c, SVC_TIME, 1) } {
        log_info(c, b"nespro time service unavailable\0");
        return;
    }
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return;
    };
    let Some(time) = (unsafe { ctx_ref.time.as_ref() }) else {
        return;
    };
    let mut id = RUN_TIMER.load(Ordering::Acquire);
    if id < 0 {
        id = unsafe { (time.timer_create)(c, TIMER_PERIOD_MS, 1, runtime_tick, null_mut()) };
        if id < 0 {
            log_info(c, b"nespro timer create failed\0");
            return;
        }
        RUN_TIMER.store(id, Ordering::Release);
    }
    let _ = unsafe { (time.timer_start)(c, id) };
}

/// Stops the pump; `destroy` releases the timer id as well.
unsafe fn stop_runtime(c: *const ContextV1, destroy: bool) {
    let id = RUN_TIMER.load(Ordering::Acquire);
    if id < 0 {
        return;
    }
    let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
        return;
    };
    let Some(time) = (unsafe { ctx_ref.time.as_ref() }) else {
        return;
    };
    let _ = unsafe { (time.timer_stop)(c, id) };
    if destroy {
        let _ = unsafe { (time.timer_destroy)(c, id) };
        RUN_TIMER.store(-1, Ordering::Release);
    }
}

extern "C" fn page_on_signal(
    _page: *mut PageDescriptor,
    _event: u32,
    _payload: *mut c_void,
) -> i32 {
    0
}

extern "C" fn page_on_create(
    _page: *mut PageDescriptor,
    root: *mut c_void,
    _start_data: *mut c_void,
) -> i32 {
    if root.is_null() {
        return -1;
    }
    unsafe {
        let content = lvx_content_create(root);
        let parent = if content.is_null() {
            root
        } else {
            lvx_object_set_size(content, CONTENT_WIDTH, CONTENT_HEIGHT);
            lvx_object_align(content, ALIGN_TOP_MID, 0, CONTENT_TOP_OFFSET);
            content
        };

        let image = lvx_image_create(parent);
        if image.is_null() {
            return -1;
        }
        init_dsc();
        lvx_image_set_src(image, &DSC[0] as *const LvImageDsc as *const c_void);
        // 1:1 pixels. Scaling to 336 wide was the bulk of the per-frame UI cost
        // and the source of the few-pixel edge offset; the image is centred
        // inside the 336-wide content area instead.
        lvx_image_set_scale(image, NES_WIDTH as i32, NES_WIDTH as i32);
        lvx_object_align(image, ALIGN_TOP_MID, 0, 0);
        IMAGE.store(image, Ordering::Release);

        let c = ctx();
        let (dw, dh) = display_resolution(c);
        if (dw, dh) == (336, 480) {
            log_info(c, b"display 336x480\0");
        } else {
            log_info(c, b"display resolution differs from 336x480\0");
        }
        // The page opens with an empty image; the timer task loads the ROM and
        // draws the first frame, one small slice at a time.
        BUDGET.store(BUDGET_START, Ordering::Release);
        LAST_PUBLISH_MS.store(0, Ordering::Release);
        SESSION_START_MS.store(0, Ordering::Release);
        TICKS.store(0, Ordering::Release);
        INSTRUCTIONS.store(0, Ordering::Release);
        FRAMES.store(0, Ordering::Release);
        PUBLISHED.store(0, Ordering::Release);
        SLICE_SUM_MS.store(0, Ordering::Release);
        SLICE_MAX_MS.store(0, Ordering::Release);
        PAGE_ACTIVE.store(1, Ordering::Release);
        start_runtime(c);
    }
    0
}

extern "C" fn page_on_resume(_page: *mut PageDescriptor) -> i32 {
    unsafe {
        PAGE_ACTIVE.store(1, Ordering::Release);
        start_runtime(ctx());
    }
    0
}

extern "C" fn page_on_pause(_page: *mut PageDescriptor) -> i32 {
    unsafe {
        PAGE_ACTIVE.store(0, Ordering::Release);
        stop_runtime(ctx(), false);
    }
    0
}

extern "C" fn page_on_destroy(_page: *mut PageDescriptor) -> i32 {
    unsafe {
        let c = ctx();
        PAGE_ACTIVE.store(0, Ordering::Release);
        stop_runtime(c, true);
        write_perf(c);
        IMAGE.store(null_mut(), Ordering::Release);
        FRAME_PENDING.store(0, Ordering::Release);
        EMU_READY = false;
        DSC_INDEX.store(0, Ordering::Release);
        HAS_FRAME.store(0, Ordering::Release);
    }
    0
}

// ---------------------------------------------------------------------------
// Module lifecycle
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn canopus_mod_prepare(c: *const ContextV1) -> i32 {
    set_ctx(c);
    0
}

#[no_mangle]
pub extern "C" fn canopus_mod_stop(c: *const ContextV1) -> i32 {
    unsafe {
        PAGE_ACTIVE.store(0, Ordering::Release);
        stop_runtime(c, true);
        EMU_READY = false;
        FRAME_PENDING.store(0, Ordering::Release);
        HAS_FRAME.store(0, Ordering::Release);
        IMAGE.store(null_mut(), Ordering::Release);
    }
    CTX.store(null_mut(), Ordering::Release);
    0
}

extern "C" fn activate(c: *const ContextV1) -> i32 {
    set_ctx(c);
    unsafe { log_info(c, b"nespro active\0") };
    0
}

extern "C" fn deactivate(_c: *const ContextV1) -> i32 {
    0
}

extern "C" fn query(w: *mut StatusWriterV1) -> i32 {
    let Some(w) = (unsafe { w.as_mut() }) else {
        return -1;
    };
    if !unsafe { status_put_u32(w, MAGIC) } {
        return -1;
    }
    status_writer_publish(w);
    0
}

/// Stage 2 runs on the Supervisor heartbeat, where a direct page switch is
/// undone as soon as the UI task next runs. Hop onto the UI task first so the
/// navigation is performed by its owner.
extern "C" fn navigate_to_app(c: *const ContextV1, _data: *mut c_void) {
    let app_id = APP_ID.load(Ordering::Acquire);
    if app_id == 0 {
        return;
    }
    unsafe {
        activity_navigate(app_id << 16, 0, 0, 0);
    }
}

extern "C" fn publish_native_app_stage(c: *const ContextV1, stage: u32) -> i32 {
    if stage == 1 {
        if !unsafe { services_has(c, SVC_LAUNCHER, 1) } {
            return -1;
        }
        let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
            return -1;
        };
        let launcher = unsafe { ctx_ref.launcher.as_ref() };
        let Some(launcher) = launcher else {
            return -1;
        };
        if APP_ID.load(Ordering::Acquire) != 0 {
            return 0;
        }
        unsafe {
            let page = &mut PAGE;
            page.page_name = PAGE_NAME.as_mut_ptr() as *mut c_void;
            page.page_id = 0;
            page.on_signal = page_on_signal as *mut c_void;
            page.on_create = page_on_create as *mut c_void;
            page.on_resume = page_on_resume as *mut c_void;
            page.on_pause = page_on_pause as *mut c_void;
            page.on_destroy = page_on_destroy as *mut c_void;

            let pages: [*mut c_void; 1] = [page as *mut PageDescriptor as *mut c_void];
            let entry = AppEntryV1 {
                struct_size: core::mem::size_of::<AppEntryV1>() as u32,
                label: b"NES\0".as_ptr() as *const c_char,
                icon: b"/resource/app/launcher/compass.bin\0".as_ptr() as *const c_char,
                pages: pages.as_ptr(),
                page_count: 1,
            };
            let app_id = (launcher.register_app)(c, &entry);
            if app_id < 0 {
                return app_id;
            }
            APP_ID.store(app_id as u32, Ordering::Release);
        }
        0
    } else if stage == 2 {
        let app_id = APP_ID.load(Ordering::Acquire);
        if app_id == 0 {
            return -1;
        }
        let Some(ctx_ref) = (unsafe { (c as *const CanopusContext).as_ref() }) else {
            return -1;
        };
        let Some(ui) = (unsafe { ctx_ref.ui.as_ref() }) else {
            return -1;
        };
        unsafe {
            if (ui.post)(c, navigate_to_app, null_mut()) < 0 {
                navigate_to_app(c, null_mut());
            }
        }
        0
    } else {
        -1
    }
}

const fn pack<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < s.len() && i < N {
        out[i] = s[i];
        i += 1;
    }
    out
}

#[no_mangle]
pub static canopus_module_descriptor: ModuleDescriptorV1 = ModuleDescriptorV1 {
    struct_size: core::mem::size_of::<ModuleDescriptorV1>() as u32,
    abi_major: ABI_MAJOR,
    abi_minor: ABI_MINOR,
    flags: FLAG_HAS_NATIVE_APP
        | FLAG_REGISTERS_LAUNCHER_ENTRY
        | FLAG_APP_UNREGISTER_REBOOT_REQUIRED,
    module_id: pack(b"nespro"),
    module_version: pack(b"0.1.2"),
    build_id: pack(b"nespro-0.1.2"),
    target_id: pack(env!("CANOPUS_TARGET_ID").as_bytes()),
    prepare: Some(canopus_mod_prepare),
    activate: Some(activate),
    deactivate: Some(deactivate),
    stop: Some(canopus_mod_stop),
    query: Some(query),
    publish_native_app: None,
    publish_native_app_stage: Some(publish_native_app_stage),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Never return into the firmware: record the fault, then fail closed and
    // let the watchdog reset the device instead of hanging the UI task.
    unsafe { log_info(ctx(), b"nespro panic\0") };
    loop {
        core::hint::spin_loop();
    }
}
