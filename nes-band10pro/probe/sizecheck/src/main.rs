use std::mem::size_of;

use nes::apu::Apu;
use nes::bus::Bus;
use nes::cpu::CpuRp2a03;
use nes::mapper::nintendo::nrom::Nrom;
use nes::mapper::Mapper;
use nes::ppu::Ppu;
use nes::rom::Rom;
use nes::Emulator;

fn main() {
    println!("Emulator   {:>8}", size_of::<Emulator>());
    println!("  Cpu      {:>8}", size_of::<CpuRp2a03>());
    println!("  Bus      {:>8}", size_of::<Bus>());
    println!("    Ppu    {:>8}", size_of::<Ppu>());
    println!("    Apu    {:>8}", size_of::<Apu>());
    println!("    Mapper {:>8}", size_of::<Mapper>());
    println!("Nrom       {:>8}", size_of::<Nrom>());
    println!("Rom        {:>8}", size_of::<Rom>());
}
