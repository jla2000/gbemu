//! Ties CPU/PPU/APU/MMU/Joypad/Serial/Timer/Cartridge together and drives
//! execution.
//!
//! `step()` runs the CPU against the MMU's bus, including interrupt
//! dispatch, the serial port's loopback output capture, the timer, the
//! PPU's dot-accurate mode sequencer, OAM DMA, the APU's channels/frame
//! sequencer/sample generation, and the cartridge's MBC3 RTC (a no-op for
//! every other MBC). The serial port, timer, PPU registers, joypad, APU
//! registers, and cartridge live on the MMU (`system.mmu.serial`,
//! `system.mmu.timer`, `system.mmu.ppu`, `system.mmu.joypad`,
//! `system.mmu.apu`, `system.mmu.cartridge`), not as separate `System`
//! fields — they're either I/O-mapped register blocks (`SB`/`SC`,
//! `DIV`/`TIMA`/`TMA`/`TAC`, `LCDC`/`STAT`/..., `JOYP`, `NR10`/...) or
//! address-range owners (ROM/cartridge RAM banking) that naturally belong
//! behind the bus that owns their range.

use crate::cpu::Cpu;
use crate::mmu::Mmu;
use crate::ppu::DOTS_PER_FRAME;

/// Top-level emulated system.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct System {
    pub cpu: Cpu,
    pub mmu: Mmu,
}

impl System {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
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
    ///
    /// This emulator has no boot ROM, so it can't reach the cartridge's
    /// entry point by actually executing one — instead this hands off
    /// straight to the CPU/PPU/APU register state a real DMG boot ROM
    /// leaves behind just before it jumps to `0x0100` (see
    /// [`Self::power_on_post_boot`]). Without this, `cpu.regs.pc` and
    /// `mmu.ppu`'s `LCDC` stay at their `Default` zero value: PC=0 (not
    /// the cartridge's real entry point) and the LCD off — and since
    /// `run_frame`/the TUI's frame loop refuse to step the CPU at all
    /// while the LCD is off, the whole system would be permanently
    /// frozen before a single instruction ran.
    pub fn load_cartridge(&mut self, data: &[u8]) -> Vec<String> {
        let warnings = self.mmu.load_cartridge(data);
        self.power_on_post_boot();
        warnings
    }

    /// Sets CPU registers and PPU/APU I/O registers to their documented
    /// DMG post-boot-ROM values (see
    /// <https://gbdev.io/pandocs/Power_Up_Sequence.html>), standing in for
    /// the boot ROM this emulator doesn't execute. APU registers are set
    /// through their normal `write_*` methods (rather than by poking
    /// fields directly) so the power-on gating in `Apu::write_nr52` runs
    /// first, same as the real boot ROM's own register-write order.
    fn power_on_post_boot(&mut self) {
        self.cpu.regs.set_af(0x01B0);
        self.cpu.regs.set_bc(0x0013);
        self.cpu.regs.set_de(0x00D8);
        self.cpu.regs.set_hl(0x014D);
        self.cpu.regs.sp = 0xFFFE;
        self.cpu.regs.pc = 0x0100;

        self.mmu.ppu.write_lcdc(0x91);
        self.mmu.ppu.write_bgp(0xFC);

        self.mmu.apu.write_nr52(0x80); // power on first: other writes are gated on it
        self.mmu.apu.write_nr10(0x80);
        self.mmu.apu.write_nr11(0xBF);
        self.mmu.apu.write_nr12(0xF3);
        self.mmu.apu.write_nr13(0xFF);
        self.mmu.apu.write_nr14(0xBF);
        self.mmu.apu.write_nr21(0x3F);
        self.mmu.apu.write_nr22(0x00);
        self.mmu.apu.write_nr23(0xFF);
        self.mmu.apu.write_nr24(0xBF);
        self.mmu.apu.write_nr30(0x7F);
        self.mmu.apu.write_nr31(0xFF);
        self.mmu.apu.write_nr32(0x9F);
        self.mmu.apu.write_nr33(0xFF);
        self.mmu.apu.write_nr34(0xBF);
        self.mmu.apu.write_nr41(0xFF);
        self.mmu.apu.write_nr42(0x00);
        self.mmu.apu.write_nr43(0x00);
        self.mmu.apu.write_nr44(0xBF);
        self.mmu.apu.write_nr50(0x77);
        self.mmu.apu.write_nr51(0xF3);
    }

    /// Execute a single CPU instruction — including interrupt dispatch —
    /// and advance other components by the elapsed T-cycles. Returns the
    /// elapsed T-cycles.
    ///
    /// The CPU runs against the MMU's bus, and the timer, PPU mode
    /// sequencer, OAM DMA, APU, and cartridge RTC all advance in step with
    /// it.
    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.mmu);
        self.mmu.step_timer(cycles);
        self.mmu.step_ppu(cycles);
        self.mmu.step_cartridge(cycles);
        self.mmu.step_dma(cycles);
        self.mmu.step_joypad();
        self.mmu.step_apu(cycles);
        cycles
    }

    /// Runs one GB frame's worth of T-cycles ([`DOTS_PER_FRAME`], ~70224 —
    /// the real DMG's dots/frame), regardless of the PPU's LCD-enabled
    /// state.
    ///
    /// This used to instead step until `LY` crossed into VBlank, bailing
    /// out immediately (without stepping the CPU at all) if the LCD was
    /// off, on the theory that a disabled LCD never reaches VBlank so
    /// waiting for one would spin forever. But the CPU doesn't stop just
    /// because LCDC's enable bit is off on real hardware, and it's
    /// completely ordinary for a ROM to disable the LCD briefly (e.g. to
    /// set up VRAM before turning it back on) — game boot sequences and
    /// test ROMs like `dmg-acid2` both do this. The old logic froze the
    /// whole system the instant that happened: once the LCD was off,
    /// every subsequent `run_frame` call would see it still off and
    /// refuse to step even once, forever. Pacing off a fixed cycle budget
    /// instead sidesteps that entirely — the CPU always keeps running,
    /// and the PPU's own mode sequencer (driven by `step_ppu` every
    /// instruction, independent of this loop) is what actually tracks
    /// `LY`/VBlank internally, whether or not the LCD is currently on.
    pub fn run_frame(&mut self) {
        let mut dots_run: u32 = 0;
        while dots_run < DOTS_PER_FRAME {
            dots_run += self.step() as u32;
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
    fn run_frame_keeps_stepping_the_cpu_even_while_the_lcd_is_off() {
        // Regression test: run_frame used to bail out without stepping the
        // CPU at all whenever the LCD was off, which froze the system
        // forever the instant any ROM disabled the LCD (completely
        // ordinary -- e.g. to set up VRAM before turning it back on).
        let mut sys = System::new();
        sys.load_rom(&[0x3C, 0xC3, 0x00, 0x00]); // loop: INC A; JP 0x0000
        sys.run_frame();
        assert_eq!(sys.mmu.ppu.read_ly(), 0); // PPU itself stays frozen (LCD off)...
        assert_ne!(sys.cpu.regs.a, 0); // ...but the CPU kept looping and incrementing A
    }

    #[test]
    fn run_frame_runs_exactly_one_frames_worth_of_dots_and_requests_vblank() {
        let mut sys = System::new();
        sys.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop, 16 T-cycles/iter)
        sys.mmu.ppu.write_lcdc(0x80); // LCD on
        sys.run_frame();
        // DOTS_PER_FRAME (70224) is an exact multiple of this loop's 16
        // T-cycles/iteration, so run_frame lands exactly back at the start
        // of the next frame (LY=0, Mode 2) rather than overshooting.
        assert_eq!(sys.mmu.ppu.read_ly(), 0);
        assert_eq!(sys.mmu.ppu.read_stat() & 0b11, 2); // Mode 2 (OAM) of the next frame
        use crate::cpu::Bus;
        assert_eq!(sys.mmu.read(0xFF0F) & 0x01, 0x01); // VBlank IF bit set during the run
    }
}
