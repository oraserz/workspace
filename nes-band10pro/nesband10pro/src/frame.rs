//! NES master-palette to RGB565 conversion.
//!
//! `x-nes` renders `Ppu::frame` as 256x240 *master palette indices*
//! (`0x00..=0x3F`); this module maps each sample to RGB565 for the firmware
//! LVGL image descriptor. It is `no_std`-clean and is also compiled by the
//! host verifier (`probe/host-render`) so both sides share one implementation.
//!
//! On device the destination lives inside the PPU frame buffer itself (there
//! is no room for a second 24 KiB buffer in the 128 KiB module heap), so the
//! conversion is alias-safe: each output row is built in a small stack row
//! buffer and only then written, and `sy` always advances faster than `y`, so
//! a written row never overwrites source rows that are still needed.

/// Emulated frame width, fixed by the PPU.
pub const SRC_WIDTH: usize = 256;
/// Emulated frame height, fixed by the PPU.
pub const SRC_HEIGHT: usize = 240;
/// Largest display width the module requests (128x96, 4:3).
pub const MAX_WIDTH: usize = 128;

/// NES 2C02 master palette (0x00..=0x3F) pre-converted to little-endian
/// RGB565 (`rrrrrggg gggbbbbb`) by `scripts/gen_palette.py`. Colours are
/// approximate NTSC renderings, which is what every software palette is.
pub const NES_PALETTE_RGB565: [u16; 64] = [
    0x630C, 0x00F6, 0x2039, 0x5019, 0x7016, 0x880E, 0xA007, 0xA080, //
    0x8960, 0x6280, 0x3360, 0x0400, 0x042A, 0x03D1, 0x0353, 0x0000, //
    0xBDF7, 0x039D, 0x415F, 0x80DF, 0xB09C, 0xD814, 0xE00B, 0xE100, //
    0xCA80, 0xAC20, 0x7540, 0x3D40, 0x15C0, 0x0588, 0x0554, 0x0000, //
    0xFFFF, 0x3DFF, 0x6C5F, 0x9BDF, 0xC3DF, 0xEB5F, 0xFAD3, 0xFACC, //
    0xFBC6, 0xFD00, 0xFEC0, 0xC700, 0x8F40, 0x578A, 0x3F95, 0x0000, //
    0xFFFF, 0xAF3F, 0xBDDF, 0xDD5F, 0xF55F, 0xFD5C, 0xFD57, 0xFD94, //
    0xFE11, 0xFECE, 0xFF4B, 0xDFCB, 0xBFCF, 0xA795, 0x9F9B, 0x0000, //
];

/// Nearest-neighbour resample of one emulated frame into `dst`, which must not
/// overlap `index_frame`. `width`/`height` describe `dst`.
// Only the host probe (probe/host-render) uses this non-aliasing entry point.
#[allow(dead_code)]
pub fn convert(index_frame: &[u8], dst: &mut [u16], width: usize, height: usize) {
    assert!(index_frame.len() >= SRC_WIDTH * SRC_HEIGHT);
    assert!(dst.len() >= width * height);
    assert!(width <= MAX_WIDTH);
    // SAFETY: the slices are valid for the stated lengths and do not overlap.
    unsafe {
        convert_raw(
            index_frame.as_ptr(),
            dst.as_mut_ptr(),
            width,
            height,
        );
    }
}

/// Alias-safe conversion used on device: `dst` may point inside `src`.
///
/// # Safety
/// `src` must be readable for `SRC_WIDTH * SRC_HEIGHT` bytes, `dst` must be
/// writable for `width * height` `u16`s and 2-byte aligned, and
/// `width <= MAX_WIDTH`.
pub unsafe fn convert_raw(src: *const u8, dst: *mut u16, width: usize, height: usize) {
    let mut row = [0u16; MAX_WIDTH];
    for y in 0..height {
        let sy = y * SRC_HEIGHT / height;
        let src_row = unsafe { src.add(sy * SRC_WIDTH) };
        for x in 0..width {
            let sx = x * SRC_WIDTH / width;
            let index = unsafe { *src_row.add(sx) } & 0x3F;
            row[x] = NES_PALETTE_RGB565[index as usize];
        }
        let dst_row = unsafe { dst.add(y * width) };
        unsafe { core::ptr::copy_nonoverlapping(row.as_ptr(), dst_row, width) };
    }
}
