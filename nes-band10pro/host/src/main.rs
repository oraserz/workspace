//! Host-side frame exporter.
//!
//! Runs the self-authored NROM test ROM through the exact `x-nes` core and the
//! exact conversion module (`nes_band/src/video.rs`, pulled in with `#[path]`)
//! that the device module uses, then writes PPM/PNG snapshots for inspection.
//!
//! The device path is: `x-nes` 256x240 palette-index frame -> `convert_nes_frame`
//! (RGB565) -> `lvx_image_set_src` on an LVGL image object that LVGL centers on
//! the 336x480 panel. The host reproduces that placement with
//! `compose_centered` so the exported screen-sized image matches the device
//! geometry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nes::rom::Rom;

#[path = "../../nes_band/src/video.rs"]
#[allow(dead_code)] // shared with the device module; not every helper is used here
mod video;

use video::{compose_centered, convert_nes_frame, centered_offset, NES_FRAME_LEN, NES_HEIGHT, NES_WIDTH};

const SCREEN_W: usize = 336;
const SCREEN_H: usize = 480;

/// Snapshot frames. `make_test_rom.py` scrolls one pixel per frame when the
/// PPU is rendering, so these show three visibly different vertical offsets.
const SNAPSHOTS: [u32; 3] = [10, 30, 60];

fn main() -> ExitCode {
    let rom_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nes_band/assets/test_band.nes".to_string());
    let out_dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "out".to_string());

    match run(Path::new(&rom_path), Path::new(&out_dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(rom_path: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(rom_path)?;
    let rom = Rom::new(&data)?;
    println!(
        "rom={} bytes={} mapper={} prg={} chr={}",
        rom_path.display(),
        data.len(),
        rom.mapper_id,
        rom.prg.len(),
        rom.chr.len()
    );
    println!(
        "emulator_bytes={} rgb565_bytes={}",
        core::mem::size_of::<nes::Emulator>(),
        NES_FRAME_LEN * 2
    );

    let mut emu = nes::Emulator::new(rom.create_mapper());
    fs::create_dir_all(out_dir)?;

    let (ox, oy) = centered_offset(SCREEN_W, SCREEN_H);
    println!(
        "panel={}x{} nes={}x{} offset=({},{})",
        SCREEN_W, SCREEN_H, NES_WIDTH, NES_HEIGHT, ox, oy
    );

    let mut rgb565 = vec![0u16; NES_FRAME_LEN];
    let mut screen = vec![0u16; SCREEN_W * SCREEN_H];
    let mut rendered_frames = 0u32;
    let mut next_snapshot = 0usize;
    let started = std::time::Instant::now();

    // Upper bound so a broken ROM cannot spin forever.
    for _ in 0..2_000_000u32 {
        let _ = emu.tick();
        if !emu.bus.ppu.frame_complete {
            continue;
        }
        emu.bus.ppu.frame_complete = false;
        rendered_frames += 1;

        if next_snapshot < SNAPSHOTS.len() && rendered_frames >= SNAPSHOTS[next_snapshot] {
            let frame = emu.bus.ppu.frame;
            convert_nes_frame(&frame, &mut rgb565);
            compose_centered(&frame, SCREEN_W, SCREEN_H, &mut screen);

            let stamp = format!("frame_{rendered_frames:04}");
            let nes_ppm = out_dir.join(format!("{stamp}_256x240.ppm"));
            let screen_ppm = out_dir.join(format!("{stamp}_{SCREEN_W}x{SCREEN_H}.ppm"));
            let screen_png = out_dir.join(format!("{stamp}_{SCREEN_W}x{SCREEN_H}.png"));

            write_ppm(&nes_ppm, NES_WIDTH, NES_HEIGHT, &screen_from_rgb565(&rgb565))?;
            write_ppm(&screen_ppm, SCREEN_W, SCREEN_H, &screen_from_rgb565(&screen))?;
            write_png(&screen_png, SCREEN_W, SCREEN_H, &screen_from_rgb565(&screen))?;

            println!(
                "frame={rendered_frames} unique_colors={} fnv={:#010x} -> {}",
                unique_colors(&frame),
                fnv1a(&frame),
                screen_png.display()
            );
            next_snapshot += 1;
        }

        if next_snapshot == SNAPSHOTS.len() {
            break;
        }
    }

    if rendered_frames == 0 {
        return Err("the ROM never completed a frame".into());
    }

    let elapsed = started.elapsed();
    println!(
        "rendered_frames={rendered_frames} host_us_per_frame={} output={}",
        elapsed.as_micros() / u128::from(rendered_frames),
        out_dir.display()
    );
    Ok(())
}

fn screen_from_rgb565(pixels: &[u16]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for &px in pixels {
        let r = ((px >> 11) & 0x1F) as u8;
        let g = ((px >> 5) & 0x3F) as u8;
        let b = (px & 0x1F) as u8;
        rgb.push((r << 3) | (r >> 2));
        rgb.push((g << 2) | (g >> 4));
        rgb.push((b << 3) | (b >> 2));
    }
    rgb
}

fn unique_colors(frame: &[u8]) -> usize {
    let mut seen = [false; 256];
    for &index in frame {
        seen[index as usize] = true;
    }
    seen.iter().filter(|v| **v).count()
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn write_ppm(path: &Path, width: usize, height: usize, rgb: &[u8]) -> std::io::Result<()> {
    let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
    out.extend_from_slice(rgb);
    fs::write(path, out)
}

/// Minimal PNG writer: 8-bit RGB, filter type 0 per scanline, zlib stream made
/// of stored (uncompressed) deflate blocks. No external crates.
fn write_png(path: &Path, width: usize, height: usize, rgb: &[u8]) -> std::io::Result<()> {
    let stride = width * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height);
    for y in 0..height {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }

    let mut zlib = Vec::with_capacity(raw.len() + 16);
    zlib.extend_from_slice(&[0x78, 0x01]); // CMF/FLG: 32K window, no dict, fastest
    let mut offset = 0usize;
    while offset < raw.len() {
        let len = (raw.len() - offset).min(0xFFFF);
        let is_final = offset + len == raw.len();
        zlib.push(u8::from(is_final));
        zlib.extend_from_slice(&(len as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(len as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, filter 0, no interlace
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);

    let _ = PathBuf::from(path);
    fs::write(path, png)
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
