//! Shared NES frame conversion: palette-indexed 256x240 -> RGB565.
//!
//! This module is `no_std` (core only) and is compiled into both the device
//! module and the host verification binary, so the host evidence exercises the
//! exact same conversion path used on the watch.

pub const NES_WIDTH: usize = 256;
pub const NES_HEIGHT: usize = 240;
pub const NES_PIXELS: usize = NES_WIDTH * NES_HEIGHT;

/// Standard 2C02 palette as 0xRRGGBB (same table as x-nes `examples/window.rs`).
pub static PALETTE_RGB: [u32; 64] = [
    0x545454, 0x001E74, 0x080090, 0x440088, 0x7C005C, 0xA4001C, 0xA80000, 0x880000, 0x5C2800,
    0x284400, 0x005400, 0x005030, 0x004444, 0x000000, 0x000000, 0x000000, 0xB4B4B4, 0x0C54C4,
    0x303CD8, 0x742CC4, 0xAC1898, 0xD8004C, 0xDC0800, 0xBC3000, 0x805000, 0x486800, 0x107800,
    0x007444, 0x00686C, 0x000000, 0x000000, 0x000000, 0xFCFCFC, 0x64B0FC, 0x9090FC, 0xC87CFC,
    0xFC74FC, 0xFC74B8, 0xFC7870, 0xFC9838, 0xF0B800, 0xBCD000, 0x84DC48, 0x58D878, 0x44D0A8,
    0x000000, 0x000000, 0x000000, 0xFCFCFC, 0xC0E4FC, 0xD0D4FC, 0xE8CCFC, 0xFCC8FC, 0xFCC4E0,
    0xFCC8B8, 0xFCD4A0, 0xFCE090, 0xE4EC88, 0xC8F090, 0xA8F0A8, 0xB0ECC8, 0x000000, 0x000000,
    0x000000,
];

/// Packs 8-bit RGB into LVGL RGB565 (little-endian value written to a `u16`).
pub const fn rgb888_to_rgb565(rgb: u32) -> u16 {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
}

/// One `u16` per pixel; index `y * 256 + x`, matching `ppu.frame`.
pub fn convert_frame(src: &[u8; NES_PIXELS], dst: &mut [u16; NES_PIXELS]) {
    for (i, &index) in src.iter().enumerate() {
        dst[i] = rgb888_to_rgb565(PALETTE_RGB[(index & 0x3F) as usize]);
    }
}

/// Nearest-neighbour scale into a `dw`x`dh` RGB565 buffer (host evidence only).
pub fn scale_rgb565(
    src: &[u16; NES_PIXELS],
    dst: &mut [u16],
    dw: usize,
    dh: usize,
) {
    for y in 0..dh {
        let sy = y * NES_HEIGHT / dh;
        for x in 0..dw {
            let sx = x * NES_WIDTH / dw;
            dst[y * dw + x] = src[sy * NES_WIDTH + sx];
        }
    }
}
