//! Stage-1 link probe: x-nes on thumbv8m.main-none-eabi.
//!
//! This is NOT the product module. It exists only to prove that x-nes and a
//! no_std staticlib can be compiled and relocatably linked for the Canopus
//! `xiaomi-band-10-pro-3.101.043` target (armv8-m.main / cortex-m33 / soft
//! float). Heap is a fixed link-time arena; the real module will route
//! `GlobalAlloc` through `ctx->mem` (see docs/DECISION.md).

#![no_std]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use nes::rom::Rom;
use nes::Emulator;

const HEAP_BYTES: usize = 192 * 1024;
const ALIGN: usize = 8;

static mut HEAP: [u8; HEAP_BYTES] = [0; HEAP_BYTES];
static USED: AtomicUsize = AtomicUsize::new(0);

struct BumpAlloc;

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(ALIGN);
        let mut offset = USED.load(Ordering::Relaxed);
        let misaligned = offset % align;
        if misaligned != 0 {
            offset += align - misaligned;
        }
        let end = match offset.checked_add(layout.size()) {
            Some(end) => end,
            None => return ptr::null_mut(),
        };
        if end > HEAP_BYTES {
            return ptr::null_mut();
        }
        USED.store(end, Ordering::Relaxed);
        unsafe { (ptr::addr_of_mut!(HEAP) as *mut u8).add(offset) }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Link probe only: never reclaimed.
    }
}

#[global_allocator]
static ALLOCATOR: BumpAlloc = BumpAlloc;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

static mut EMULATOR: Option<Emulator> = None;

/// Parse `data` as iNES and construct the emulator. Returns 0 on success.
#[no_mangle]
pub extern "C" fn xnes_probe_boot(data: *const u8, len: usize) -> i32 {
    if data.is_null() || len < 16 {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    let Ok(rom) = Rom::new(bytes) else {
        return -2;
    };
    let emulator = Emulator::new(rom.create_mapper());
    unsafe {
        EMULATOR = Some(emulator);
    }
    0
}

/// Run CPU instructions until the PPU completes one frame, then return the
/// 256x240 palette-index frame pointer. Returns null when not booted.
#[no_mangle]
pub extern "C" fn xnes_probe_frame() -> *const u8 {
    let Some(emulator) = (unsafe { (ptr::addr_of_mut!(EMULATOR) as *mut Option<Emulator>).as_mut() })
    else {
        return ptr::null();
    };
    let Some(emulator) = emulator.as_mut() else {
        return ptr::null();
    };
    let mut guard = 0u32;
    while !emulator.bus.ppu.frame_complete {
        emulator.tick();
        guard = guard.wrapping_add(1);
        if guard == 0 {
            return ptr::null();
        }
    }
    emulator.bus.ppu.frame_complete = false;
    emulator.bus.ppu.frame.as_ptr()
}
