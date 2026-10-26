//! Host-side frame verifier.
//!
//! Runs the generated NROM test ROM through x-nes, then converts the PPU
//! palette-index frame with the *same* `frame.rs` that the device module uses.
//! Writes PPM snapshots and prints a summary so the "frame -> screen" path is
//! proven outside the firmware (device verification is still pending).

use std::fs;
use std::path::PathBuf;

use nes::{rom::Rom, Emulator};

// The device module's conversion code, compiled unchanged for the host.
#[allow(dead_code)]
#[path = "../../../nesband10pro/src/frame.rs"]
mod frame;

const WIDTH: usize = 128;
const HEIGHT: usize = 96;

fn main() {
    let rom_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/pattern.nes".to_string());
    let out_dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "build/host-render".to_string());
    let out_dir = PathBuf::from(out_dir);
    fs::create_dir_all(&out_dir).expect("create output dir");

    let bytes = fs::read(&rom_path).expect("read test ROM");
    let rom = Rom::new(&bytes).expect("parse iNES ROM");
    assert_eq!(rom.mapper_id, 0, "test ROM must be NROM (mapper 0)");
    let mapper = rom.create_mapper();
    drop(rom);
    let mut emulator = Emulator::new(mapper);

    let mut pixels = vec![0u16; WIDTH * HEIGHT];
    let mut frame_no = 0u32;
    let mut total_cycles = 0u64;
    let mut snapshots = 0u32;

    // Run up to eight frames; snapshot frames 1, 2 and 8.
    while frame_no < 8 {
        emulator.bus.ppu.frame_complete = false;
        let mut cycles = 0u32;
        while !emulator.bus.ppu.frame_complete {
            cycles += u32::from(emulator.tick());
        }
        total_cycles += u64::from(cycles);
        frame_no += 1;

        if frame_no == 1 || frame_no == 2 || frame_no == 8 {
            frame::convert(&emulator.bus.ppu.frame, &mut pixels, WIDTH, HEIGHT);
            let path = out_dir.join(format!("frame{frame_no}.ppm"));
            write_ppm(&path, &pixels);
            fs::write(
                out_dir.join(format!("frame{frame_no}.indices")),
                emulator.bus.ppu.frame,
            )
            .expect("write raw indices");
            println!(
                "frame {frame_no}: {cycles} cycles, unique colours {} -> {}",
                unique_colours(&emulator.bus.ppu.frame),
                unique_rgb565(&pixels)
            );
            snapshots += 1;
        }
    }

    // The generated ROM paints a 4x4-tile checkerboard of tile ids 1/2/3,
    // i.e. NES palette entries 0x16 / 0x2A / 0x12 (red / yellow / blue).
    let ppu = &emulator.bus.ppu;
    println!(
        "ppu: mask={:#04x} ctrl={:#04x} v={:#06x} t={:#06x} render_v={:#06x} fine_x={}",
        ppu.mask, ppu.ctrl, ppu.v, ppu.t, ppu.render_v, ppu.render_fine_x
    );

    // Re-run the device's in-place path: source frame buffer with an odd start
    // address (the real PPU field sits at byte 57), destination one byte in.
    let mut storage = vec![0u8; 57 + 256 * 240];
    let src = unsafe { storage.as_mut_ptr().add(57) };
    unsafe { std::ptr::copy_nonoverlapping(emulator.bus.ppu.frame.as_ptr(), src, 256 * 240) };
    let dst = unsafe { src.add((src as usize) & 1) };
    unsafe { frame::convert_raw(src, dst.cast::<u16>(), WIDTH, HEIGHT) };
    let in_place = unsafe { std::slice::from_raw_parts(dst, WIDTH * HEIGHT * 2) };
    let separate = unsafe {
        std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), WIDTH * HEIGHT * 2)
    };
    let alias_ok = in_place == separate;
    println!("in-place conversion matches separate buffer: {alias_ok}");
    if !alias_ok {
        std::process::exit(3);
    }

    let check = check_checkerboard(&emulator.bus.ppu.frame);
    println!(
        "frames={frame_no} snapshots={snapshots} cycles/frame={} check={check}",
        total_cycles / u64::from(frame_no)
    );
    if !check {
        std::process::exit(2);
    }
}

fn write_ppm(path: &std::path::Path, pixels: &[u16]) {
    let mut out = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    out.reserve(WIDTH * HEIGHT * 3);
    for &p in pixels {
        let r = ((p >> 11) & 0x1F) as u8;
        let g = ((p >> 5) & 0x3F) as u8;
        let b = (p & 0x1F) as u8;
        out.push((r << 3) | (r >> 2));
        out.push((g << 2) | (g >> 4));
        out.push((b << 3) | (b >> 2));
    }
    fs::write(path, out).expect("write PPM");
}

fn unique_colours(frame: &[u8]) -> usize {
    let mut seen = [false; 64];
    for &index in frame.iter() {
        seen[(index & 0x3F) as usize] = true;
    }
    seen.iter().filter(|v| **v).count()
}

fn unique_rgb565(pixels: &[u16]) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for &p in pixels {
        seen.insert(p);
    }
    seen.len()
}

/// Every 4x4 tile block is a solid colour; the block colours cycle 1,2,3.
fn check_checkerboard(frame: &[u8]) -> bool {
    for block_y in 0..(240 / 32) {
        for block_x in 0..(256 / 32) {
            let expected = [0x16u8, 0x2A, 0x12][(block_x + block_y) % 3];
            for y in 0..32 {
                for x in 0..32 {
                    let index = frame[(block_y * 32 + y) * 256 + block_x * 32 + x] & 0x3F;
                    if index != expected {
                        eprintln!(
                            "mismatch at block ({block_x},{block_y}) pixel ({x},{y}): {index:#04x} != {expected:#04x}"
                        );
                        return false;
                    }
                }
            }
        }
    }
    true
}
