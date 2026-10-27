//! Native app registration for the NES page: one app, one page, published in
//! the Supervisor's two stages (app + pages, then the Launcher entry).

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use canopus_target_private::*;

use super::ui;

pub const APP_ID: u16 = 0x00D1;
pub const PAGE_COUNT: usize = 1;

pub const PACKAGE_NAME: &[u8] = b"com.canopus.nes-band\0";
pub const DISPLAY_NAME: &[u8] = b"NES\0";
/// Supplied next to the package as `assets/appicon_nes.bin`; the installer
/// places it here, like LyraPlayer's watchface does for its icon.
pub const LAUNCHER_ICON: &[u8] = b"/data/canopus/appicon_nes.bin\0";
const PAGE_NAME: &[u8] = b"nes_band_main\0";

const STATE_NONE: u32 = 0;
const STATE_REGISTERED: u32 = 1;
const STATE_PUBLISHED: u32 = 2;

static APP_STATE: AtomicU32 = AtomicU32::new(STATE_NONE);
static APP_INSTALL_RESULT: AtomicI32 = AtomicI32::new(0);
static LAUNCHER_RESULT: AtomicI32 = AtomicI32::new(0);

static mut APP_DESCRIPTOR: core::mem::MaybeUninit<launcher_app_descriptor> =
    core::mem::MaybeUninit::uninit();
static mut PAGE_DESCRIPTORS: core::mem::MaybeUninit<[firmware_page_descriptor; PAGE_COUNT]> =
    core::mem::MaybeUninit::uninit();

fn page_descriptor(index: usize) -> *mut firmware_page_descriptor {
    // SAFETY: the array is a module-owned static initialized before install.
    unsafe {
        core::ptr::addr_of_mut!(PAGE_DESCRIPTORS)
            .cast::<firmware_page_descriptor>()
            .add(index)
    }
}

extern "C" fn launcher_display_name() -> *const u8 {
    DISPLAY_NAME.as_ptr()
}

fn c_str_equal(a: *const u8, expected: &[u8]) -> bool {
    if a.is_null() || expected.last() != Some(&0) {
        return false;
    }
    let mut i = 0usize;
    while i < expected.len() {
        if unsafe { *a.add(i) } != expected[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn package_matches(app: *mut core::ffi::c_void) -> bool {
    if app.is_null() {
        return false;
    }
    // SAFETY: the firmware app record carries the package pointer at the exact
    // 3.101.043 offset.
    let package: *const u8 =
        unsafe { core::ptr::read(app.cast::<u8>().add(APP_PACKAGE_OFFSET) as *const *const u8) };
    c_str_equal(package, PACKAGE_NAME)
}

fn app_descriptor_init() {
    // SAFETY: module-private static initialized once before `app_install`.
    unsafe {
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(APP_DESCRIPTOR).cast::<u8>(),
            0,
            core::mem::size_of::<launcher_app_descriptor>(),
        );
        let app = &mut *core::ptr::addr_of_mut!(APP_DESCRIPTOR).cast::<launcher_app_descriptor>();
        app.package_name = PACKAGE_NAME.as_ptr() as *mut core::ffi::c_void;
        app.launcher_icon_resource = LAUNCHER_ICON.as_ptr() as *mut core::ffi::c_void;
        app.app_id = APP_ID;
        app.launcher_metadata_callback = launcher_display_name as *const () as *mut core::ffi::c_void;
    }
}

fn descriptor_init(index: usize) {
    let descriptor = page_descriptor(index);
    // SAFETY: zero-valid region of the static array; the firmware only reads it
    // after `app_install` returns.
    unsafe {
        core::ptr::write_bytes(
            descriptor.cast::<u8>(),
            0,
            core::mem::size_of::<firmware_page_descriptor>(),
        );
        (*descriptor).page_name = PAGE_NAME.as_ptr() as *mut core::ffi::c_void;
        (*descriptor).page_id = index as u16;
        (*descriptor).app_id = APP_ID;
        (*descriptor).on_signal = page_on_signal as *const () as *mut core::ffi::c_void;
        (*descriptor).on_create = page_on_create as *const () as *mut core::ffi::c_void;
        (*descriptor).on_resume = page_on_resume as *const () as *mut core::ffi::c_void;
        (*descriptor).on_pause = page_on_pause as *const () as *mut core::ffi::c_void;
        // 3.101.043 uses `on_destroy`; the .139/.155 pages use `on_ui_destroy`.
        (*descriptor).on_destroy = page_on_destroy as *const () as *mut core::ffi::c_void;
    }
}

pub fn install_stage(stage: u32) -> Result<(), i32> {
    let existing = unsafe { app_lookup(APP_ID) };

    if stage == 1 {
        if !existing.is_null() {
            if !package_matches(existing) {
                return Err(-101);
            }
            APP_STATE.store(STATE_REGISTERED, Ordering::Release);
            return Ok(());
        }

        app_descriptor_init();
        descriptor_init(0);

        let pages: [*mut firmware_page_descriptor; PAGE_COUNT] = [page_descriptor(0)];
        // SAFETY: descriptors are fully initialized above; `app_install`
        // consumes the pointer array synchronously and retains the descriptors.
        let install_result = unsafe {
            app_install(
                core::ptr::addr_of_mut!(APP_DESCRIPTOR).cast::<launcher_app_descriptor>(),
                pages.as_ptr(),
                PAGE_COUNT as u32,
            )
        };
        APP_INSTALL_RESULT.store(install_result, Ordering::Release);

        let installed = unsafe { app_lookup(APP_ID) };
        if !package_matches(installed) {
            return Err(-100);
        }
        APP_STATE.store(STATE_REGISTERED, Ordering::Release);
        return Ok(());
    }

    if stage == 2 {
        if existing.is_null() || !package_matches(existing) {
            return Err(-102);
        }
        if APP_STATE.load(Ordering::Acquire) == STATE_PUBLISHED {
            return Ok(());
        }
        // `launcher_add` returns launcher bookkeeping, not a status code; the
        // app lookup above is the validity gate (same rule as LyraPlayer).
        let launcher_result = unsafe { launcher_add(APP_ID) };
        LAUNCHER_RESULT.store(launcher_result, Ordering::Release);
        APP_STATE.store(STATE_PUBLISHED, Ordering::Release);
        return Ok(());
    }

    Err(-103)
}

fn page_id_of(page: *mut firmware_page_descriptor) -> usize {
    if page.is_null() {
        return usize::MAX;
    }
    // SAFETY: the firmware passes one of the descriptors registered above.
    usize::from(unsafe { (*page).page_id })
}

extern "C" fn page_on_signal(
    _page: *mut firmware_page_descriptor,
    _event: u32,
    _payload: *mut core::ffi::c_void,
) -> i32 {
    0
}

extern "C" fn page_on_create(
    page: *mut firmware_page_descriptor,
    root: *mut core::ffi::c_void,
    _start_data: *mut core::ffi::c_void,
) -> i32 {
    if page_id_of(page) >= PAGE_COUNT {
        return -1;
    }
    ui::page_create(root)
}

extern "C" fn page_on_resume(page: *mut firmware_page_descriptor) -> i32 {
    if page_id_of(page) >= PAGE_COUNT {
        return -1;
    }
    ui::page_resume()
}

extern "C" fn page_on_pause(page: *mut firmware_page_descriptor) -> i32 {
    if page_id_of(page) >= PAGE_COUNT {
        return -1;
    }
    ui::page_pause()
}

extern "C" fn page_on_destroy(page: *mut firmware_page_descriptor) -> i32 {
    if page_id_of(page) >= PAGE_COUNT {
        return -1;
    }
    ui::page_destroy()
}

/// `canopus_mod_query` tail: app state, install/launcher results.
pub fn query_status() -> [u32; 3] {
    [
        APP_STATE.load(Ordering::Acquire),
        APP_INSTALL_RESULT.load(Ordering::Acquire) as u32,
        LAUNCHER_RESULT.load(Ordering::Acquire) as u32,
    ]
}
