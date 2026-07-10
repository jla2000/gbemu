//! Ties CPU/PPU/APU/MMU/Joypad/Serial/Timer/Cartridge together and drives
//! execution.
//!
//! `step()` runs the CPU against the MMU's bus, including interrupt
//! dispatch, the serial port's loopback output capture, the timer, the
//! PPU's dot-accurate mode sequencer, OAM DMA, and the cartridge's MBC3
//! RTC (a no-op for every other MBC). The serial port, timer, PPU
//! registers, joypad, and cartridge live on the MMU (`system.mmu.serial`,
//! `system.mmu.timer`, `system.mmu.ppu`, `system.mmu.joypad`,
//! `system.mmu.cartridge`), not as separate `System` fields — they're
//! either I/O-mapped register blocks (`SB`/`SC`, `DIV`/`TIMA`/`TMA`/`TAC`,
//! `LCDC`/`STAT`/..., `JOYP`) or address-range owners (ROM/cartridge RAM
//! banking) that naturally belong behind the bus that owns their range,
//! same as APU registers will be once those land. APU stepping by elapsed
//! T-cycles lands in M5.

use crate::apu::Apu;
use crate::cpu::Cpu;
use crate::mmu::Mmu;
use crate::ppu::VBLANK_START_LINE;

/// Top-level emulated system.
#[derive(Debug, Default)]
pub struct System {
    pub cpu: Cpu,
    pub apu: Apu,
    pub mmu: Mmu,
}

impl System {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            apu: Apu::new(),
            mmu: Mmu::new(),
        }
    }

    /// Load raw ROM bytes directly into the MMU's flat address space, no
    /// cartridge/MBC banking. Used by the Blargg harness and CPU-core unit
    /// tests that feed it small synthetic byte sequences with no real
    /// cartridge header — see [`System::load_cartridge`] for real ROM
    /// files.
    pub fn load_rom(&mut self, data: &[u8]) {
        self.mmu.load_rom(data);
    }

    /// Parses and installs `data` as a real cartridge (header, MBC,
    /// banking), replacing any previous cartridge or flat-loaded ROM.
    /// Returns non-fatal header-validation warnings.
    pub fn load_cartridge(&mut self, data: &[u8]) -> Vec<String> {
        self.mmu.load_cartridge(data)
    }

    /// Execute a single CPU instruction — including interrupt dispatch —
    /// and advance other components by the elapsed T-cycles. Returns the
    /// elapsed T-cycles.
    ///
    /// APU is still a placeholder (lands in M5) so it is not yet advanced
    /// here; the CPU already runs against the MMU's bus, and the
    /// timer, PPU mode sequencer, OAM DMA, and cartridge RTC already
    /// advance in step with it.
    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.mmu);
        self.mmu.step_timer(cycles);
        self.mmu.step_ppu(cycles);
        self.mmu.step_cartridge(cycles);
        self.mmu.step_dma(cycles);
        self.mmu.step_joypad();
        cycles
    }

    /// Run until the PPU signals VBlank start (`LY` reaches
    /// [`VBLANK_START_LINE`]), i.e. one full frame. Returns immediately if
    /// the LCD is off, since a disabled LCD never reaches VBlank.
    ///
    /// No framebuffer is produced yet (BG/window/sprite fetch lands later
    /// in M2) — this only drives the mode sequencer far enough for
    /// interrupt-driven test ROMs and the eventual render loop to rely on.
    pub fn run_frame(&mut self) {
        if !self.mmu.ppu.lcd_enabled() {
            return;
        }
        loop {
            let ly_before = self.mmu.ppu.read_ly();
            self.step();
            let ly_after = self.mmu.ppu.read_ly();
            if ly_before != VBLANK_START_LINE && ly_after == VBLANK_START_LINE {
                break;
            }
            if !self.mmu.ppu.lcd_enabled() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_executes_one_instruction_via_mmu() {
        let mut sys = System::new();
        sys.load_rom(&[0x3E, 0x2A, 0x06, 0x10]); // LD A,0x2A; LD B,0x10
        let t1 = sys.step();
        assert_eq!(sys.cpu.regs.a, 0x2A);
        assert_eq!(t1, 8);
        let t2 = sys.step();
        assert_eq!(sys.cpu.regs.b, 0x10);
        assert_eq!(t2, 8);
        assert_eq!(sys.cpu.regs.pc, 4);
    }

    #[test]
    fn run_frame_returns_immediately_when_lcd_is_off() {
        let mut sys = System::new();
        sys.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop)
        sys.run_frame();
        assert_eq!(sys.mmu.ppu.read_ly(), 0); // never ticked
    }

    #[test]
    fn run_frame_stops_at_vblank_and_requests_the_interrupt() {
        let mut sys = System::new();
        sys.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop)
        sys.mmu.ppu.write_lcdc(0x80); // LCD on
        sys.run_frame();
        assert_eq!(sys.mmu.ppu.read_ly(), VBLANK_START_LINE);
        assert_eq!(sys.mmu.ppu.read_stat() & 0b11, 1); // Mode 1 (VBlank)
        use crate::cpu::Bus;
        assert_eq!(sys.mmu.read(0xFF0F) & 0x01, 0x01); // VBlank IF bit set
    }
}
