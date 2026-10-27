//! NES frame conversion shared by the device module and the host exporter.
//!
//! `x-nes` renders a 256x240 buffer of PPU palette indices (0..63). The device
//! hands LVGL an RGB565 image of exactly that size at 1:1 scale, centered by
//! LVGL on the 336x480 panel. The host exporter calls the same
//! [`convert_nes_frame`] so both paths produce identical pixels.

pub const NES_WIDTH: usize = 256;
pub const NES_HEIGHT: usize = 240;
pub const NES_FRAME_LEN: usize = NES_WIDTH * NES_HEIGHT;

/// One `u16` RGB565 pixel per NES pixel (the device image buffer layout).
pub const RGB565_BYTES: usize = NES_FRAME_LEN * 2;

/// LVGL v9 `LV_COLOR_FORMAT_RGB565`.
pub const LV_COLOR_FORMAT_RGB565: u8 = 0x12;

/// Classic 2C02 palette (RGB888), indexed exactly like NES PPU palette RAM.
pub const NES_PALETTE: [(u8, u8, u8); 64] = [
    (84, 84, 84),
    (0, 30, 116),
    (8, 16, 144),
    (48, 0, 136),
    (68, 0, 100),
    (92, 0, 48),
    (84, 4, 0),
    (60, 24, 0),
    (32, 42, 0),
    (8, 58, 0),
    (0, 64, 0),
    (0, 60, 0),
    (0, 50, 60),
    (0, 0, 0),
    (0, 0, 0),
    (0, 0, 0),
    (152, 150, 152),
    (8, 76, 196),
    (48, 50, 236),
    (92, 30, 228),
    (136, 20, 176),
    (160, 20, 100),
    (152, 34, 32),
    (120, 60, 0),
    (84, 90, 0),
    (40, 114, 0),
    (8, 124, 0),
    (0, 118, 40),
    (0, 102, 120),
    (0, 0, 0),
    (0, 0, 0),
    (0, 0, 0),
    (236, 238, 236),
    (76, 154, 236),
    (120, 124, 236),
    (176, 98, 236),
    (228, 84, 236),
    (236, 88, 180),
    (236, 106, 100),
    (212, 136, 32),
    (160, 170, 0),
    (116, 196, 0),
    (76, 208, 32),
    (56, 204, 108),
    (56, 180, 204),
    (60, 60, 60),
    (0, 0, 0),
    (0, 0, 0),
    (236, 238, 236),
    (168, 204, 236),
    (188, 188, 236),
    (212, 178, 236),
    (236, 174, 236),
    (236, 174, 212),
    (236, 180, 176),
    (228, 196, 144),
    (204, 210, 120),
    (180, 222, 120),
    (168, 226, 144),
    (152, 226, 180),
    (160, 214, 228),
    (160, 162, 160),
    (0, 0, 0),
    (0, 0, 0),
];

#[inline]
pub const fn palette_rgb(index: u8) -> (u8, u8, u8) {
    NES_PALETTE[(index & 0x3F) as usize]
}

#[inline]
pub const fn rgb565(red: u8, green: u8, blue: u8) -> u16 {
    (((red as u16) & 0xF8) << 8) | (((green as u16) & 0xFC) << 3) | ((blue as u16) >> 3)
}

#[inline]
pub const fn palette_rgb565(index: u8) -> u16 {
    let (red, green, blue) = palette_rgb(index);
    rgb565(red, green, blue)
}

/// Converts a 256x240 palette-index frame into the RGB565 image buffer the
/// device hands to LVGL. `out.len()` must be [`NES_FRAME_LEN`] or larger.
pub fn convert_nes_frame(frame: &[u8], out: &mut [u16]) {
    let count = frame.len().min(out.len());
    for i in 0..count {
        out[i] = palette_rgb565(frame[i]);
    }
}

/// Top-left offset of the 1:1 image centered inside a `screen_w`x`screen_h`
/// panel (the same geometry `lvx_object_align(.., CENTER)` produces).
pub const fn centered_offset(screen_w: usize, screen_h: usize) -> (usize, usize) {
    let x = screen_w.saturating_sub(NES_WIDTH) / 2;
    let y = screen_h.saturating_sub(NES_HEIGHT) / 2;
    (x, y)
}

/// Composes the centered 1:1 image with black borders into a screen-sized
/// RGB565 buffer. Host-preview only; the device lets LVGL perform the same
/// placement.
pub fn compose_centered(frame: &[u8], screen_w: usize, screen_h: usize, out: &mut [u16]) {
    let black = rgb565(0, 0, 0);
    for pixel in out.iter_mut() {
        *pixel = black;
    }
    let (offset_x, offset_y) = centered_offset(screen_w, screen_h);
    for y in 0..NES_HEIGHT {
        let dst_y = offset_y + y;
        if dst_y >= screen_h {
            break;
        }
        for x in 0..NES_WIDTH {
            let dst_x = offset_x + x;
            if dst_x >= screen_w {
                break;
            }
            let index = y * NES_WIDTH + x;
            if index < frame.len() {
                out[dst_y * screen_w + dst_x] = palette_rgb565(frame[index]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_entries_fit_rgb565() {
        for index in 0u8..64 {
            let (red, green, blue) = palette_rgb(index);
            assert_eq!(rgb565(red, green, blue) & 0xF800, u16::from(red & 0xF8) << 8);
            assert!(green < 256 && blue < 256);
        }
    }

    #[test]
    fn centered_offsets_match_the_band_10_pro_panel() {
        assert_eq!(centered_offset(336, 480), (40, 120));
    }

    #[test]
    fn compose_keeps_black_borders() {
        let mut frame = [0u8; NES_FRAME_LEN];
        frame[0] = 0x30; // white
        let mut screen = [0u16; 336 * 480];
        compose_centered(&frame, 336, 480, &mut screen);
        assert_eq!(screen[0], rgb565(0, 0, 0));
        assert_eq!(screen[120 * 336 + 40], rgb565(236, 238, 236));
        assert_eq!(screen[479 * 336 + 335], rgb565(0, 0, 0));
    }
}
