//! LVGL side of the NES page: one RGB565 image object that is re-pointed at a
//! freshly emulated frame from an LVGL timer on the UI owner thread.
//!
//! `lv_image_set_src` is DEVICE_PROVEN for file paths (LyraPlayer). This module
//! passes an in-memory `lv_image_dsc_t` instead, which is the standard LVGL v9
//! variable-image source; that variant is host/static evidence only until it
//! is probed on the device (see `REPORT.md`).

use core::ffi::c_void;

use canopus_target_private::{
    lvx_content_create, lvx_image_create, lvx_image_set_src, lvx_object_align, lvx_object_delete,
    lvx_object_set_size, lvx_timer_create, lvx_timer_delete, ALIGN_TOP_MID, CONTENT_WIDTH, LvxTimerCallback,
};

use crate::machine;
use crate::video::{LV_COLOR_FORMAT_RGB565, NES_HEIGHT, NES_WIDTH};

/// Panel geometry. `configure` overwrites these with the live resolution read
/// from `ctx->display->get_resolution`; the initializers are the Band 10 Pro
/// panel (336x480), where the 256x240 image is centered with its top edge at
/// y = (480 - 240) / 2 = 120.
static mut PANEL_WIDTH: i32 = CONTENT_WIDTH;
static mut PANEL_HEIGHT: i32 = 480;
static mut IMAGE_TOP: i32 = (480 - NES_HEIGHT as i32) / 2;

/// Called once from `canopus_mod_activate`, before any page exists. `None`
/// keeps the Band 10 Pro fallback above.
pub fn configure(screen: Option<(u32, u32)>) {
    let Some((width, height)) = screen else {
        return;
    };
    if width == 0 || height == 0 {
        return;
    }
    // SAFETY: activation runs on one thread and before the page is created.
    unsafe {
        PANEL_WIDTH = width as i32;
        PANEL_HEIGHT = height as i32;
        IMAGE_TOP = ((height as i32 - NES_HEIGHT as i32) / 2).max(0);
    }
}

/// One emulated frame per timer tick. The callback runs on the UI thread, so
/// LVGL may be touched from here. 100 ms matches the refresh cadence the
/// LyraPlayer module uses on this target and leaves the UI thread responsive;
/// the host measures ~2.7 ms per frame in release on x86, but the same work on
/// the M33 has not been timed yet (see `REPORT.md`).
const FRAME_PERIOD_MS: u32 = 100;

/// LVGL v9 `lv_image_dsc_t` for a raw in-memory image.
#[repr(C)]
struct ImageDsc {
    magic: u8,
    color_format: u8,
    flags: u16,
    width: u16,
    height: u16,
    stride: u16,
    reserved: u16,
    data_size: u32,
    data: *const u8,
    reserved_ptr: *const c_void,
}

const EMPTY_DSC: ImageDsc = ImageDsc {
    magic: 0x19,
    color_format: LV_COLOR_FORMAT_RGB565,
    flags: 0,
    width: NES_WIDTH as u16,
    height: NES_HEIGHT as u16,
    stride: (NES_WIDTH * 2) as u16,
    reserved: 0,
    data_size: (NES_WIDTH * NES_HEIGHT * 2) as u32,
    data: core::ptr::null(),
    reserved_ptr: core::ptr::null(),
};

/// Two descriptors over the same pixel buffer. Handing LVGL the other one each
/// frame guarantees `lv_image_set_src` sees a new source and invalidates the
/// object instead of short-circuiting on an unchanged pointer.
static mut DESCRIPTORS: [ImageDsc; 2] = [EMPTY_DSC, EMPTY_DSC];
static mut DESCRIPTOR_INDEX: usize = 0;

static mut CONTENT: *mut c_void = core::ptr::null_mut();
static mut IMAGE: *mut c_void = core::ptr::null_mut();
static mut TIMER: *mut c_void = core::ptr::null_mut();
static mut VISIBLE: bool = false;

fn descriptor(index: usize) -> *mut ImageDsc {
    // SAFETY: module-owned static array; callers only write the pixel pointer.
    unsafe {
        core::ptr::addr_of_mut!(DESCRIPTORS)
            .cast::<ImageDsc>()
            .add(index)
    }
}

extern "C" fn frame_timer(_timer: *mut c_void) {
    // SAFETY: single UI thread; all pointers are module-owned statics.
    unsafe {
        if !VISIBLE || IMAGE.is_null() {
            return;
        }
        if !machine::run_frame() {
            return;
        }
        let pixels = machine::pixels();
        let next = DESCRIPTOR_INDEX ^ 1;
        let dsc = descriptor(next);
        (*dsc).data = pixels;
        lvx_image_set_src(IMAGE, dsc.cast());
        DESCRIPTOR_INDEX = next;
    }
}

/// Called from `firmware_page_descriptor::on_create` with the page root.
pub fn page_create(root: *mut c_void) -> i32 {
    if root.is_null() || !machine::is_ready() {
        return -1;
    }
    // SAFETY: LVX calls run on the page owner thread inside `on_create`.
    unsafe {
        let content = lvx_content_create(root);
        if content.is_null() {
            return -1;
        }
        let panel_width = PANEL_WIDTH;
        let panel_height = PANEL_HEIGHT;
        let image_top = IMAGE_TOP;
        lvx_object_set_size(content, panel_width, panel_height);
        lvx_object_align(content, ALIGN_TOP_MID, 0, 0);

        let image = lvx_image_create(content);
        if image.is_null() {
            lvx_object_delete(content);
            return -1;
        }
        lvx_object_set_size(image, NES_WIDTH as i32, NES_HEIGHT as i32);
        lvx_object_align(image, ALIGN_TOP_MID, 0, image_top);

        let pixels = machine::pixels();
        let dsc = descriptor(DESCRIPTOR_INDEX);
        (*dsc).data = pixels;
        lvx_image_set_src(image, dsc.cast());

        let callback: LvxTimerCallback = frame_timer;
        let timer = lvx_timer_create(callback, FRAME_PERIOD_MS, core::ptr::null_mut());
        if timer.is_null() {
            lvx_object_delete(content);
            return -1;
        }

        CONTENT = content;
        IMAGE = image;
        TIMER = timer;
        VISIBLE = true;
    }
    0
}

pub fn page_resume() -> i32 {
    // SAFETY: module-owned statics, UI thread.
    unsafe {
        if IMAGE.is_null() {
            return -1;
        }
        VISIBLE = true;
    }
    0
}

pub fn page_pause() -> i32 {
    // SAFETY: module-owned statics, UI thread.
    unsafe {
        VISIBLE = false;
    }
    0
}

pub fn page_destroy() -> i32 {
    // SAFETY: module-owned statics, UI thread; deleting the content root also
    // releases the image child.
    unsafe {
        VISIBLE = false;
        if !TIMER.is_null() {
            lvx_timer_delete(TIMER);
            TIMER = core::ptr::null_mut();
        }
        if !CONTENT.is_null() {
            lvx_object_delete(CONTENT);
            CONTENT = core::ptr::null_mut();
        }
        IMAGE = core::ptr::null_mut();
    }
    0
}
