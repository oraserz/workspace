//! nespro — NES emulator display module for Xiaomi Band 10 Pro (043).
//!
//! The module registers a native launcher application. When its page is created
//! it loads a ROM (or the embedded NROM test ROM), runs the `x-nes` core on a
//! page-owned LVGL timer, converts each 256x240 palette-indexed frame to
//! RGB565, and refreshes a target-bound `lv_image` widget backed by a module
//! `lv_image_dsc_t`.
//!
//! Evidence level: compiled and ELF-verified only. The firmware-private LVGL
//! entry points used here are STATIC_RECOVERED, not device-verified.

#![no_std]

extern crate alloc;

mod nes_frame;

use alloc::vec::Vec;
use canopus_abi::*;
use canopus_runtime::{mem, status_put_u32, status_writer_publish};
use canopus_target_private::{activity_navigate, lvx_content_create, lvx_image_create,
    lvx_image_set_scale, lvx_image_set_src, lvx_object_align, lvx_object_set_size,
    lvx_timer_create, lvx_timer_delete, ALIGN_TOP_MID, CONTENT_HEIGHT, CONTENT_TOP_OFFSET,
    CONTENT_WIDTH};
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_void};
use core::mem::MaybeUninit;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU8, Ordering};
use nes::Emulator;
use nes::rom::Rom;
use nes_frame::{NES_HEIGHT, NES_PIXELS, NES_WIDTH, convert_frame};

const MAGIC: u32 = 0x4E455350; // "NESP"
const ROM_PATH: &[u8] = b"/data/canopus/nespro.nes\0";
const TIMER_PERIOD_MS: u32 = 33;
/// Upper bound of instructions executed per UI timer tick (~one NES frame).
const INSTRUCTION_BUDGET: u32 = 120_000;

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
static TIMER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

// ---------------------------------------------------------------------------
// Emulator state
// ---------------------------------------------------------------------------

static mut EMU: MaybeUninit<Emulator> = MaybeUninit::uninit();
static mut EMU_READY: bool = false;

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

unsafe fn read_rom_from_fs(c: *const ContextV1) -> Option<Vec<u8>> {
    if !unsafe { services_has(c, SVC_FS, 1) } {
        return None;
    }
    let ctx_ref = unsafe { (c as *const CanopusContext).as_ref() }?;
    let fs = unsafe { ctx_ref.fs.as_ref() }?;
    let path = ROM_PATH.as_ptr() as *const c_char;
    let mut stat = CanopusFsStat {
        size: 0,
        is_dir: 0,
        _pad: [0; 3],
        mtime: 0,
    };
    if unsafe { (fs.exists)(c, path) } <= 0 {
        return None;
    }
    if unsafe { (fs.stat)(c, path, &mut stat) } != 0 || stat.size == 0 {
        return None;
    }
    let mut buf: Vec<u8> = alloc::vec![0u8; stat.size as usize];
    let read = unsafe { (fs.read_all)(c, path, buf.as_mut_ptr() as *mut c_void, stat.size) };
    if read <= 0 {
        return None;
    }
    buf.truncate(read as usize);
    Some(buf)
}

unsafe fn start_emulator(rom_bytes: &[u8]) -> bool {
    let Ok(rom) = Rom::new(rom_bytes) else {
        return false;
    };
    let mapper = rom.create_mapper();
    // The mapper owns its PRG/CHR copy; release `rom` before allocating the
    // 74 KiB emulator so peak heap use stays inside the 128 KiB module heap.
    drop(rom);
    let emu = Emulator::new(mapper);
    unsafe {
        EMU.as_mut_ptr().write(emu);
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

extern "C" fn frame_timer(_timer: *mut c_void) {
    unsafe {
        if !EMU_READY {
            return;
        }
        let emu = &mut *EMU.as_mut_ptr();
        let mut budget = INSTRUCTION_BUDGET;
        while !emu.bus.ppu.frame_complete && budget > 0 {
            let _ = emu.tick();
            budget -= 1;
        }
        emu.bus.ppu.frame_complete = false;
        convert_frame(&emu.bus.ppu.frame, &mut FB);

        let image = IMAGE.load(Ordering::Acquire);
        if image.is_null() {
            return;
        }
        let next = 1 - DSC_INDEX.load(Ordering::Relaxed);
        DSC_INDEX.store(next, Ordering::Relaxed);
        let dsc = &DSC[next as usize] as *const LvImageDsc as *const c_void;
        lvx_image_set_src(image, dsc);
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
        // 336/256 = 1.3125 -> a 336x315 picture inside the 336x424 content area.
        lvx_image_set_scale(image, CONTENT_WIDTH, CONTENT_WIDTH);
        lvx_object_align(image, ALIGN_TOP_MID, 0, 0);
        IMAGE.store(image, Ordering::Release);

        let c = ctx();
        let (dw, dh) = display_resolution(c);
        if (dw, dh) == (336, 480) {
            log_info(c, b"display 336x480\0");
        } else {
            log_info(c, b"display resolution differs from 336x480\0");
        }
        let rom = read_rom_from_fs(c);
        let started = match rom.as_deref() {
            Some(bytes) => start_emulator(bytes),
            None => start_emulator(TEST_ROM),
        };
        if !started {
            return -1;
        }
        // Draw the first frame immediately so the page is never blank.
        frame_timer(null_mut());

        let timer = lvx_timer_create(frame_timer, TIMER_PERIOD_MS, null_mut());
        TIMER.store(timer, Ordering::Release);
    }
    0
}

extern "C" fn page_on_resume(_page: *mut PageDescriptor) -> i32 {
    0
}

extern "C" fn page_on_pause(_page: *mut PageDescriptor) -> i32 {
    0
}

extern "C" fn page_on_destroy(_page: *mut PageDescriptor) -> i32 {
    unsafe {
        let timer = TIMER.swap(null_mut(), Ordering::AcqRel);
        if !timer.is_null() {
            lvx_timer_delete(timer);
        }
        IMAGE.store(null_mut(), Ordering::Release);
        EMU_READY = false;
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
pub extern "C" fn canopus_mod_stop(_c: *const ContextV1) -> i32 {
    unsafe {
        EMU_READY = false;
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
        unsafe {
            activity_navigate(app_id << 16, 0, 0, 0);
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
    module_version: pack(b"0.1.0"),
    build_id: pack(b"nespro-0.1.0"),
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
    loop {}
}
