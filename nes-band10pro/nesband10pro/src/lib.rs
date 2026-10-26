//! nesband10pro — NES emulator (x-nes) for the Xiaomi Band 10 Pro, firmware
//! 3.101.043.
//!
//! Target-bound Canopus module. The frame path is a firmware LVGL image object
//! fed by a module-owned `lv_image_dsc_t`; the semantic UI ABI's IMAGE node is
//! path-driven and rebuilt per snapshot, so it cannot carry video.
//!
//! Lifecycle constraints (see docs/02-implementation.md):
//! - `prepare` runs on the stock `insmod` 7.9 KiB stack: no allocation.
//! - `activate` builds the emulator on the module heap (<= 128 KiB budget).
//! - The module is removable; pages are owned by the firmware.

#![no_std]

extern crate alloc;

mod frame;
mod nes;
mod ui;

use canopus_abi::*;
use canopus_runtime::{status_put_u32, status_writer_publish};
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_void};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

const MAGIC: u32 = 0x4E45_5331; // "NES1"

/// The module context, so the allocator and timers can reach their services.
static CONTEXT: AtomicPtr<CanopusContext> = AtomicPtr::new(ptr::null_mut());
/// Zero while active; a negative load/init code otherwise.
static LOAD_STATE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn context<'a>(ctx: *const ContextV1) -> Option<&'a CanopusContext> {
    let ctx = unsafe { (ctx as *const CanopusContext).as_ref() }?;
    if (ctx.struct_size as usize) < core::mem::size_of::<CanopusContext>() {
        return None;
    }
    Some(ctx)
}

/// Binds the module allocator. Allocation-free: safe on the 7.9 KiB stack.
fn bind_context(ctx: *const ContextV1) {
    if !ctx.is_null() {
        CONTEXT.store(ctx.cast::<CanopusContext>() as *mut CanopusContext, Ordering::Release);
    }
}

pub(crate) fn now_ms() -> u32 {
    let ctx = CONTEXT.load(Ordering::Acquire);
    if ctx.is_null() {
        return 0;
    }
    let Some(c) = context(ctx.cast::<ContextV1>()) else {
        return 0;
    };
    if c.time.is_null() {
        return 0;
    }
    unsafe { ((*c.time).monotonic_ms)(ctx.cast::<ContextV1>()) }
}

// ---------------------------------------------------------------------------
// Module heap: `ctx->mem` is the Supervisor's per-module pool (128 KiB).
// ---------------------------------------------------------------------------

struct ModuleAllocator;

/// `ctx->mem->alloc` has no alignment parameter; NuttX `malloc` returns
/// 8-byte-aligned blocks, which is the alignment every type here needs.
const ALLOCATOR_ALIGN: usize = 8;

unsafe impl GlobalAlloc for ModuleAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 || layout.align() > ALLOCATOR_ALIGN {
            return ptr::null_mut();
        }
        let ctx = CONTEXT.load(Ordering::Acquire);
        if ctx.is_null() {
            return ptr::null_mut();
        }
        let Some(c) = context(ctx.cast::<ContextV1>()) else {
            return ptr::null_mut();
        };
        if c.mem.is_null() {
            return ptr::null_mut();
        }
        unsafe { ((*c.mem).alloc)(ctx.cast::<ContextV1>(), layout.size() as u32).cast() }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let ctx = CONTEXT.load(Ordering::Acquire);
        if ctx.is_null() || ptr.is_null() {
            return;
        }
        let Some(c) = context(ctx.cast::<ContextV1>()) else {
            return;
        };
        if c.mem.is_null() {
            return;
        }
        unsafe { ((*c.mem).free)(ctx.cast::<ContextV1>(), ptr.cast::<c_void>()) };
    }
}

#[global_allocator]
static ALLOCATOR: ModuleAllocator = ModuleAllocator;

// ---------------------------------------------------------------------------
// Lifecycle callbacks
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn canopus_mod_prepare(ctx: *const ContextV1) -> i32 {
    bind_context(ctx);
    0
}

extern "C" fn activate(ctx: *const ContextV1) -> i32 {
    bind_context(ctx);
    if ctx.is_null() {
        return -22;
    }
    let rc = nes::init(ctx);
    LOAD_STATE.store(rc as u32, Ordering::Release);
    if let Some(c) = context(ctx) {
        if !c.log.is_null() {
            let message: &[u8] = if rc == 0 {
                b"nesband10pro: emulator ready\0"
            } else {
                b"nesband10pro: no ROM / no memory\0"
            };
            unsafe { ((*c.log).info)(ctx, message.as_ptr() as *const c_char) };
        }
    }
    0
}

extern "C" fn deactivate(_ctx: *const ContextV1) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn canopus_mod_stop(_ctx: *const ContextV1) -> i32 {
    0
}

extern "C" fn query(writer: *mut StatusWriterV1) -> i32 {
    let Some(writer) = (unsafe { writer.as_mut() }) else {
        return -1;
    };
    let (width, height) = nes::dimensions();
    let values = [
        MAGIC,
        LOAD_STATE.load(Ordering::Acquire),
        u32::from(nes::ready()),
        nes::frames(),
        nes::last_frame_ms(),
        width,
        height,
        heap_used(),
        heap_available(),
    ];
    for value in values {
        if !unsafe { status_put_u32(writer, value) } {
            return -1;
        }
    }
    status_writer_publish(writer);
    0
}

fn heap_used() -> u32 {
    let ctx = CONTEXT.load(Ordering::Acquire);
    let Some(c) = context(ctx.cast::<ContextV1>()) else {
        return 0;
    };
    if c.mem.is_null() {
        return 0;
    }
    unsafe { ((*c.mem).used)(ctx.cast::<ContextV1>()) }
}

fn heap_available() -> u32 {
    let ctx = CONTEXT.load(Ordering::Acquire);
    let Some(c) = context(ctx.cast::<ContextV1>()) else {
        return 0;
    };
    if c.mem.is_null() {
        return 0;
    }
    unsafe { ((*c.mem).available)(ctx.cast::<ContextV1>()) }
}

extern "C" fn publish_native_app(_ctx: *const ContextV1) -> i32 {
    // Only the staged path is implemented (stage 1 app/pages, stage 2 launcher).
    -103
}

extern "C" fn publish_native_app_stage(_ctx: *const ContextV1, stage: u32) -> i32 {
    ui::install_stage(stage)
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
        | FLAG_NATIVE_APP_STANDALONE
        | FLAG_REGISTERS_LAUNCHER_ENTRY
        | FLAG_REQUIRES_UI_DISPATCHER
        | FLAG_APP_UNREGISTER_REBOOT_REQUIRED,
    module_id: pack(b"nesband10pro"),
    module_version: pack(b"0.1.0"),
    build_id: pack(b"nesband10pro-0.1.0"),
    // An exact target id, or "portable-thumbv8m"; set by `canopus build`.
    target_id: pack(env!("CANOPUS_TARGET_ID").as_bytes()),
    prepare: Some(canopus_mod_prepare),
    activate: Some(activate),
    deactivate: Some(deactivate),
    stop: Some(canopus_mod_stop),
    query: Some(query),
    publish_native_app: Some(publish_native_app),
    publish_native_app_stage: Some(publish_native_app_stage),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
