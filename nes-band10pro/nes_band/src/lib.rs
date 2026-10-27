//! `nes_band` — a Canopus target module for the Xiaomi Band 10 Pro (3.101.043)
//! that emulates a NES (x-nes core) and draws its 256x240 picture on the
//! display as an LVGL RGB565 image.
//!
//! The `canopus build` C shim registers [`canopus_module_descriptor`] through
//! `sdk/c/canopus_module_entry.h` (CMR1 write on the stock modlib loader), so
//! the module itself only provides the descriptor and the lifecycle hooks.
#![cfg_attr(feature = "device", no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "device")]
extern crate alloc;

#[cfg(feature = "device")]
mod allocator;
#[cfg(feature = "device")]
mod app;
#[cfg(feature = "device")]
mod machine;
#[cfg(feature = "device")]
mod ui;

#[cfg(feature = "device")]
#[global_allocator]
static ALLOCATOR: allocator::FirmwareAllocator = allocator::FirmwareAllocator;

pub mod video;

use canopus_abi::*;
use canopus_runtime::{status_put_u32, status_writer_publish};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const MAGIC: u32 = 0x3153_454E; // "NES1"

static ACTIVE: AtomicBool = AtomicBool::new(false);
static RESIDENT: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

const fn pack<const N: usize>(value: &[u8]) -> [u8; N] {
    let mut output = [0; N];
    let mut index = 0;
    while index < value.len() && index < N {
        output[index] = value[index];
        index += 1;
    }
    output
}

const TARGET_ID: &[u8] = match option_env!("CANOPUS_TARGET_ID") {
    Some(target) => target.as_bytes(),
    None => b"host-test",
};

/// Allocation-free: the stock `insmod` task calls this on a 7.9 KiB stack.
#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_prepare(_ctx: *const ContextV1) -> i32 {
    ACTIVE.store(false, Ordering::Release);
    RESIDENT.store(false, Ordering::Release);
    LAST_ERROR.store(0, Ordering::Release);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_activate(ctx: *const ContextV1) -> i32 {
    #[cfg(feature = "device")]
    let result = {
        let result = machine::init();
        if result == 0 {
            ui::configure(screen_size(ctx));
        }
        result
    };
    #[cfg(not(feature = "device"))]
    let result = -1;
    let _ = ctx;
    if result == 0 {
        ACTIVE.store(true, Ordering::Release);
        RESIDENT.store(true, Ordering::Release);
    } else {
        LAST_ERROR.store(result as u32, Ordering::Release);
    }
    result
}

/// Reads the live panel geometry through `ctx->display->get_resolution`;
/// `None` falls back to the Band 10 Pro panel inside `ui::configure`.
fn screen_size(ctx: *const ContextV1) -> Option<(u32, u32)> {
    if ctx.is_null() {
        return None;
    }
    // SAFETY: the Supervisor passes its own context; the display object is
    // part of the ABI v1 service table.
    unsafe {
        let context = (ctx as *const CanopusContext).as_ref()?;
        if context.display.is_null() {
            return None;
        }
        let mut width = 0u32;
        let mut height = 0u32;
        ((*context.display).get_resolution)(ctx, &mut width, &mut height);
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return None;
        }
        Some((width, height))
    }
}

/// Resident after activation: LVX objects and the emulator heap can only be
/// released by the reboot the Supervisor performs after `REBOOT_REQUIRED`.
#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_deactivate(_ctx: *const ContextV1) -> i32 {
    if RESIDENT.load(Ordering::Acquire) {
        RESULT_REBOOT_REQUIRED as i32
    } else {
        ACTIVE.store(false, Ordering::Release);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_stop(ctx: *const ContextV1) -> i32 {
    canopus_mod_deactivate(ctx)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_query(writer: *mut StatusWriterV1) -> i32 {
    if writer.is_null() {
        return -1;
    }
    let writer = unsafe { &mut *writer };
    unsafe {
        if !status_put_u32(writer, MAGIC)
            || !status_put_u32(writer, ACTIVE.load(Ordering::Acquire) as u32)
            || !status_put_u32(writer, RESIDENT.load(Ordering::Acquire) as u32)
            || !status_put_u32(writer, LAST_ERROR.load(Ordering::Acquire))
        {
            return -1;
        }
        #[cfg(feature = "device")]
        for value in machine::query_status() {
            if !status_put_u32(writer, value) {
                return -1;
            }
        }
        #[cfg(feature = "device")]
        for value in app::query_status() {
            if !status_put_u32(writer, value) {
                return -1;
            }
        }
    }
    status_writer_publish(writer);
    0
}

/// The app is published in two stages; the single call form is not supported.
#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_publish_native_app(_ctx: *const ContextV1) -> i32 {
    -103
}

#[unsafe(no_mangle)]
pub extern "C" fn canopus_mod_publish_native_app_stage(_ctx: *const ContextV1, stage: u32) -> i32 {
    #[cfg(feature = "device")]
    let result = app::install_stage(stage)
        .map(|()| 0)
        .unwrap_or_else(|error| error);
    #[cfg(not(feature = "device"))]
    let result = -103;
    if result != 0 {
        LAST_ERROR.store(result as u32, Ordering::Release);
    }
    result
}

#[unsafe(no_mangle)]
pub static canopus_module_descriptor: ModuleDescriptorV1 = ModuleDescriptorV1 {
    struct_size: core::mem::size_of::<ModuleDescriptorV1>() as u32,
    abi_major: ABI_MAJOR,
    abi_minor: ABI_MINOR,
    flags: FLAG_HAS_NATIVE_APP
        | FLAG_NATIVE_APP_STANDALONE
        | FLAG_REGISTERS_LAUNCHER_ENTRY
        | FLAG_REQUIRES_UI_DISPATCHER
        | FLAG_APP_UNREGISTER_REBOOT_REQUIRED,
    module_id: pack(b"nes_band"),
    module_version: pack(b"0.1.0"),
    build_id: pack(b"nes-band-0.1.0"),
    target_id: pack(TARGET_ID),
    prepare: Some(canopus_mod_prepare),
    activate: Some(canopus_mod_activate),
    deactivate: Some(canopus_mod_deactivate),
    stop: Some(canopus_mod_stop),
    query: Some(canopus_mod_query),
    publish_native_app: Some(canopus_mod_publish_native_app),
    publish_native_app_stage: Some(canopus_mod_publish_native_app_stage),
};

#[cfg(all(feature = "device", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
