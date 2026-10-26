//! On-device presentation: native-app registration plus the LVGL image that
//! carries the emulated frame.
//!
//! Evidence levels (see docs/02-implementation.md):
//! - `app_install` / `app_lookup` / `launcher_add`, `lvx_image_create`,
//!   `lvx_image_set_src`, `lvx_image_set_scale`, object size/align and the
//!   image-descriptor field offsets: STATIC_RECOVERED for target 043
//!   (`EVID-P67-BATTERY-TRANSPARENCY-001`, `EVID-UI-1043-DEVICE-001`).
//! - `LV_COLOR_FORMAT_RGB565 == 0x12`: upstream LVGL v9 enum, NOT yet proven
//!   on this firmware; device probe required.

use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use canopus_target_private::*;

use crate::nes;

pub const APP_ID: u16 = 0x4E45; // "NE"
const PACKAGE_NAME: &[u8] = b"com.canopus.nesband10pro\0";
const DISPLAY_NAME: &[u8] = b"NES\0";
/// Firmware-owned launcher icon (same built-in the SDK demo points at); the
/// descriptor requires a non-zero resource path.
const LAUNCHER_ICON: &[u8] = b"/resource/app/launcher/compass.bin\0";
const PAGE_NAME: &[u8] = b"nes_player\0";
const PACKAGE_OFFSET: usize = 8; // launcher_app_descriptor.package_name

/// Width of the page content column; the image is stretched to it.
const VIEW_WIDTH: i32 = 336;
/// Frames are 4:3, so the widget height is 3/4 of the width.
const VIEW_HEIGHT: i32 = 252;
const CONTENT_TOP: i32 = 56;
const CONTENT_HEIGHT: i32 = 424;

/// LVGL v9 `lv_image_header_t` as recovered for this firmware: magic, format,
/// flags u16, then w/h/stride/reserved at +4/+6/+8/+10 (12 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct ImageDsc {
    magic: u8,
    cf: u8,
    flags: u16,
    width: u16,
    height: u16,
    stride: u16,
    reserved: u16,
    data_size: u32,
    data: *const u8,
    user_data: *const c_void,
}

const _: () = assert!(core::mem::size_of::<ImageDsc>() == 24);
const _: () = assert!(core::mem::offset_of!(ImageDsc, data_size) == 12);
const _: () = assert!(core::mem::offset_of!(ImageDsc, data) == 16);
const _: () = assert!(core::mem::offset_of!(ImageDsc, user_data) == 20);

/// `LV_IMAGE_HEADER_MAGIC` (LVGL v9).
const IMAGE_MAGIC: u8 = 0x19;
/// `LV_COLOR_FORMAT_RGB565` (LVGL v9 upstream); firmware enum not yet probed.
const COLOR_FORMAT_RGB565: u8 = 0x12;
/// `lvx_image_set_scale` is Q8: 256 == 100%.
const SCALE_UNIT: u32 = 256;

/// Two descriptors over one shared pixel buffer. They alternate as LVGL image
/// sources so each `lv_image_set_src` sees a different pointer and re-reads the
/// header / invalidates the widget; the pixels themselves stay in place.
static mut DESC: [ImageDsc; 2] = [EMPTY_DSC; 2];

const EMPTY_DSC: ImageDsc = ImageDsc {
    magic: IMAGE_MAGIC,
    cf: COLOR_FORMAT_RGB565,
    flags: 0,
    width: 0,
    height: 0,
    stride: 0,
    reserved: 0,
    data_size: 0,
    data: ptr::null(),
    user_data: ptr::null(),
};

static IMAGE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static TIMER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DESCRIPTORS_READY: AtomicBool = AtomicBool::new(false);
/// Alternating source pointer forces `lv_image_set_src` to re-read the header
/// and invalidate the widget even though the pixel buffer address is stable.
static SOURCE_INDEX: AtomicU32 = AtomicU32::new(0);

static mut APP_DESCRIPTOR: MaybeUninit<launcher_app_descriptor> = MaybeUninit::uninit();
static mut PAGE_DESCRIPTOR: MaybeUninit<firmware_page_descriptor> = MaybeUninit::uninit();

// ---------------------------------------------------------------------------
// Native app registration
// ---------------------------------------------------------------------------

extern "C" fn launcher_display_name() -> *const u8 {
    DISPLAY_NAME.as_ptr()
}

fn app_descriptor_ptr() -> *mut launcher_app_descriptor {
    // SAFETY: module-private static, written once by `app_descriptor_init`.
    ptr::addr_of_mut!(APP_DESCRIPTOR).cast::<launcher_app_descriptor>()
}

fn page_descriptor_ptr() -> *mut firmware_page_descriptor {
    // SAFETY: module-private static, written once by `page_descriptor_init`.
    ptr::addr_of_mut!(PAGE_DESCRIPTOR).cast::<firmware_page_descriptor>()
}

fn app_descriptor_init() {
    let app = app_descriptor_ptr();
    // SAFETY: fresh static, zeroed then filled before `app_install` sees it.
    unsafe {
        ptr::write_bytes(
            app.cast::<u8>(),
            0,
            core::mem::size_of::<launcher_app_descriptor>(),
        );
        (*app).package_name = PACKAGE_NAME.as_ptr() as *mut c_void;
        (*app).launcher_icon_resource = LAUNCHER_ICON.as_ptr() as *mut c_void;
        (*app).app_id = APP_ID;
        (*app).launcher_metadata_callback =
            launcher_display_name as *const () as *mut c_void;
    }
}

fn page_descriptor_init() {
    let page = page_descriptor_ptr();
    // SAFETY: fresh static, zeroed then filled before `app_install` sees it.
    unsafe {
        ptr::write_bytes(
            page.cast::<u8>(),
            0,
            core::mem::size_of::<firmware_page_descriptor>(),
        );
        (*page).page_name = PAGE_NAME.as_ptr() as *mut c_void;
        (*page).page_id = 0;
        (*page).app_id = APP_ID;
        (*page).on_signal = page_on_signal as *const () as *mut c_void;
        (*page).on_create = page_on_create as *const () as *mut c_void;
        (*page).on_resume = page_on_resume as *const () as *mut c_void;
        (*page).on_pause = page_on_pause as *const () as *mut c_void;
        (*page).on_destroy = page_on_destroy as *const () as *mut c_void;
    }
}

fn package_matches(app: *mut c_void) -> bool {
    if app.is_null() {
        return false;
    }
    // SAFETY: the firmware returns an installed `launcher_app_descriptor`; the
    // package pointer sits at +8 in the exact 043 layout.
    let package: *const u8 = unsafe { ptr::read(app.cast::<u8>().add(PACKAGE_OFFSET).cast()) };
    if package.is_null() {
        return false;
    }
    let mut i = 0usize;
    while i < PACKAGE_NAME.len() {
        if unsafe { *package.add(i) } != PACKAGE_NAME[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Stage 1 registers the app and its single page; stage 2 adds the Launcher
/// entry after miwear has processed the app-registry event.
pub fn install_stage(stage: u32) -> i32 {
    let existing = unsafe { app_lookup(APP_ID) };
    match stage {
        1 => {
            if !existing.is_null() {
                return if package_matches(existing) { 0 } else { -101 };
            }
            app_descriptor_init();
            page_descriptor_init();
            let pages = [page_descriptor_ptr()];
            // SAFETY: descriptors are fully initialised; `app_install` copies
            // the pointer array and retains the descriptors.
            let rc = unsafe { app_install(app_descriptor_ptr(), pages.as_ptr(), 1) };
            if rc < 0 {
                return rc;
            }
            let installed = unsafe { app_lookup(APP_ID) };
            if installed.is_null() {
                return -100;
            }
            if !package_matches(installed) {
                return -101;
            }
            0
        }
        2 => {
            if existing.is_null() {
                return -102;
            }
            if !package_matches(existing) {
                return -101;
            }
            // The return value is launcher bookkeeping, not a status code.
            unsafe { launcher_add(APP_ID) };
            0
        }
        _ => -103,
    }
}

// ---------------------------------------------------------------------------
// Page lifecycle
// ---------------------------------------------------------------------------

extern "C" fn page_on_signal(
    _page: *mut firmware_page_descriptor,
    _event: u32,
    _payload: *mut c_void,
) -> i32 {
    0
}

extern "C" fn page_on_create(
    _page: *mut firmware_page_descriptor,
    root: *mut c_void,
    _start_data: *mut c_void,
) -> i32 {
    if root.is_null() {
        return -22;
    }
    if !nes::ready() {
        show_status(root, error_text());
        return 0;
    }
    init_descriptors();
    // SAFETY: UI owner thread; root is the page's root object.
    unsafe {
        let image = lvx_image_create(root);
        if image.is_null() {
            return -1;
        }
        IMAGE.store(image, Ordering::Release);
    }
    set_source();
    start_timer();
    0
}

extern "C" fn page_on_resume(_page: *mut firmware_page_descriptor) -> i32 {
    if nes::ready() {
        start_timer();
    }
    0
}

extern "C" fn page_on_pause(_page: *mut firmware_page_descriptor) -> i32 {
    stop_timer();
    0
}

extern "C" fn page_on_destroy(_page: *mut firmware_page_descriptor) -> i32 {
    stop_timer();
    IMAGE.store(ptr::null_mut(), Ordering::Release);
    0
}

fn start_timer() {
    if !TIMER.load(Ordering::Acquire).is_null() {
        return;
    }
    // SAFETY: UI owner thread; `on_timer` is a plain C callback.
    let timer = unsafe { lvx_timer_create(on_timer, nes::PERIOD_MS, ptr::null_mut()) };
    TIMER.store(timer, Ordering::Release);
}

fn stop_timer() {
    let timer = TIMER.swap(ptr::null_mut(), Ordering::AcqRel);
    if !timer.is_null() {
        // SAFETY: created by `start_timer` on this thread.
        unsafe { lvx_timer_delete(timer) };
    }
}

extern "C" fn on_timer(_timer: *mut c_void) {
    let start = nes::now_ms();
    if nes::step() {
        nes::convert();
        set_source();
        nes::note_frame(nes::now_ms().wrapping_sub(start));
    }
}

// ---------------------------------------------------------------------------
// Image descriptor and refresh
// ---------------------------------------------------------------------------

fn init_descriptors() {
    if DESCRIPTORS_READY.load(Ordering::Acquire) {
        return;
    }
    let pixels = nes::pixel_buffer();
    let (width, height) = nes::dimensions();
    if pixels.is_null() || width == 0 || height == 0 {
        return;
    }
    // SAFETY: static array of plain data; `DESC` has interior mutability only
    // through this one-time initialisation on the UI thread.
    unsafe {
        let descs = ptr::addr_of_mut!(DESC).cast::<ImageDsc>();
        for index in 0..2 {
            let dsc = descs.add(index);
            (*dsc).width = width as u16;
            (*dsc).height = height as u16;
            (*dsc).stride = (width * 2) as u16;
            (*dsc).data_size = width * height * 2;
            (*dsc).data = pixels;
        }
    }
    DESCRIPTORS_READY.store(true, Ordering::Release);
}

fn set_source() {
    let image = IMAGE.load(Ordering::Acquire);
    if image.is_null() || !DESCRIPTORS_READY.load(Ordering::Acquire) {
        return;
    }
    let index = (SOURCE_INDEX.fetch_xor(1, Ordering::AcqRel) & 1) as usize;
    let (width, height) = nes::dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let scale = VIEW_WIDTH as u32 * SCALE_UNIT / width;
    let image_y = CONTENT_TOP + (CONTENT_HEIGHT - VIEW_HEIGHT) / 2;
    // SAFETY: UI owner thread; all objects and descriptors are module-owned.
    unsafe {
        let dsc = ptr::addr_of!(DESC).cast::<ImageDsc>().add(index);
        lvx_image_set_src(image, dsc.cast::<c_void>());
        lvx_image_set_scale(image, scale as i32, scale as i32);
        lvx_object_set_size(image, VIEW_WIDTH, VIEW_HEIGHT);
        lvx_object_align(image, ALIGN_TOP_MID, 0, image_y);
    }
}

fn error_text() -> &'static [u8] {
    match nes::status() {
        -2 => b"NES: ROM not found\0",
        -12 => b"NES: out of memory\0",
        _ => b"NES: init failed\0",
    }
}

fn show_status(root: *mut c_void, text: &'static [u8]) {
    // SAFETY: UI owner thread; label is appended to the page root.
    unsafe {
        let label = lvx_label_create(root);
        if label.is_null() {
            return;
        }
        lvx_label_set_text(label, text.as_ptr());
        lvx_object_align(label, ALIGN_TOP_MID, 0, CONTENT_TOP + 120);
    }
}
