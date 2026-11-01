//! Host-side verification for the nespro display path.
//!
//! Runs the same x-nes core and the same frame conversion module that the
//! device module uses, then writes PPM captures. This is host evidence only:
//! it does not prove anything about the watch.

#[path = "../../nespro/src/nes_frame.rs"]
mod nes_frame;

use nes::Emulator;
use nes::rom::Rom;
use nes_frame::{NES_HEIGHT, NES_PIXELS, NES_WIDTH, convert_frame, rgb888_to_rgb565};
use std::fs;

fn write_ppm(path: &str, pixels: &[u16], w: usize, h: usize) {
    let mut out = Vec::with_capacity(w * h * 3 + 32);
    out.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
    for &p in pixels {
        let r = ((p >> 11) & 0x1F) as u32 * 255 / 31;
        let g = ((p >> 5) & 0x3F) as u32 * 255 / 63;
        let b = (p & 0x1F) as u32 * 255 / 31;
        out.push(r as u8);
        out.push(g as u8);
        out.push(b as u8);
    }
    fs::write(path, out).expect("write ppm");
}

fn main() {
    let rom_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "roms/nespro-test.nes".to_string());
    let out_dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "hostverify/out".to_string());
    fs::create_dir_all(&out_dir).expect("create out dir");

    let data = fs::read(&rom_path).expect("read rom");
    let rom = Rom::new(&data).expect("parse rom");
    let mut emu = Emulator::new(rom.create_mapper());

    const W: usize = 336;
    const H: usize = 315;
    let mut scaled = vec![0u16; W * H];

    let target_frames = 4usize;
    let mut frames = 0usize;
    let mut instructions = 0usize;
    while frames < target_frames && instructions < 20_000_000 {
        emu.tick();
        instructions += 1;
        if emu.bus.ppu.frame_complete {
            emu.bus.ppu.frame_complete = false;
            frames += 1;
            let mut rgb = [0u16; NES_PIXELS];
            convert_frame(&emu.bus.ppu.frame, &mut rgb);
            let ppm = format!("{}/frame-{:02}.ppm", out_dir, frames);
            write_ppm(&ppm, &rgb, NES_WIDTH, NES_HEIGHT);
            nes_frame::scale_rgb565(&rgb, &mut scaled, W, H);
            let ppm_scaled = format!("{}/screen-{:02}.ppm", out_dir, frames);
            write_ppm(&ppm_scaled, &scaled, W, H);
        }
    }
    assert!(frames == target_frames, "emulator did not produce frames");

    // Diagnostics: PPU state after the run (host-side only).
    let ppu = &emu.bus.ppu;
    println!(
        "ppu mask={:#04x} ctrl={:#04x} vram[0x3c0..0x3d0]={:02x?} palette={:02x?}",
        ppu.mask,
        ppu.ctrl,
        &ppu.vram[0x3C0..0x3D0],
        &ppu.palette[..16],
    );
    println!("nt0[0..0x40]={:02x?}", &ppu.vram[0..0x40]);
    println!("nt0[0x100..0x140]={:02x?}", &ppu.vram[0x100..0x140]);


    // Device path preview: letterbox the 336x315 image into the 336x424 page
    // content area, exactly as the module places it (top-aligned, black fill).
    let mut content = vec![rgb888_to_rgb565(0x000000); 336 * 424];
    for y in 0..H {
        content[y * 336..y * 336 + 336].copy_from_slice(&scaled[y * 336..y * 336 + 336]);
    }
    write_ppm(
        &format!("{}/content-424.ppm", out_dir),
        &content,
        336,
        424,
    );
    println!(
        "frames={} instructions={} rom={} out={}",
        frames, instructions, rom_path, out_dir
    );
    println!(
        "mem: size_of::<Emulator>()={} size_of::<Bus>()={} rom_prg={} rom_chr={}",
        std::mem::size_of::<nes::Emulator>(),
        std::mem::size_of::<nes::bus::Bus>(),
        rom.prg.len(),
        rom.chr.len()
    );
}
