//! Ties CPU/PPU/APU/MMU/Joypad/Serial/Timer together and drives execution.
//!
//! `step()` runs the CPU against the MMU's flat 64KB bus (M1), including
//! interrupt dispatch, the serial port's loopback output capture, the
//! timer, and now the PPU's dot-accurate mode sequencer. The serial port,
//! timer, and PPU registers live on the MMU (`system.mmu.serial`,
//! `system.mmu.timer`, `system.mmu.ppu`), not as separate `System` fields —
//! they're I/O-mapped register blocks (`SB`/`SC`, `DIV`/`TIMA`/`TMA`/`TAC`,
//! `LCDC`/`STAT`/...) owned by whatever owns the address space, same as
//! APU/Joypad registers will be once those land. Actual pixel
//! rendering (BG/window/sprite fetch) and APU stepping by elapsed T-cycles
//! land in M2/M5 onward.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
use crate::joypad::Joypad;
use crate::mmu::Mmu;
use crate::ppu::VBLANK_START_LINE;

/// Top-level emulated system.
#[derive(Debug, Default)]
pub struct System {
    pub cpu: Cpu,
    pub apu: Apu,
    pub mmu: Mmu,
    pub joypad: Joypad,
    pub cartridge: Option<Cartridge>,
}

impl System {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            apu: Apu::new(),
            mmu: Mmu::new(),
            joypad: Joypad::new(),
            cartridge: None,
        }
    }

    /// Load raw ROM bytes directly into the MMU's flat address space (no
    /// cartridge/MBC banking yet — lands in M3). Enough to boot Blargg test
    /// ROMs against the M1 CPU core.
    pub fn load_rom(&mut self, data: &[u8]) {
        self.mmu.load_rom(data);
    }

    /// Execute a single CPU instruction — including interrupt dispatch —
    /// and advance other components by the elapsed T-cycles. Returns the
    /// elapsed T-cycles.
    ///
    /// APU is still a placeholder (lands in M5) so it is not yet advanced
    /// here; the CPU already runs against the MMU's flat 64KB bus, and the
    /// timer and PPU mode sequencer already advance in step with it.
    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.mmu);
        self.mmu.step_timer(cycles);
        self.mmu.step_ppu(cycles);
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
