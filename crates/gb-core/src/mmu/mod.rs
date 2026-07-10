//! Memory management unit: bus, memory map, I/O register dispatch.
//!
//! The general address space is still a flat 64KB byte array for anything
//! not routed elsewhere (WRAM/HRAM/echo RAM — no real behavioral
//! difference from flat memory yet). Real routing exists for the regions
//! that need actual side effects or backing storage beyond "a byte":
//!
//! - `SB`/`SC` (0xFF01/0xFF02), `DIV`/`TIMA`/`TMA`/`TAC` (0xFF04-0xFF07),
//!   `JOYP` (0xFF00), the PPU register block (`LCDC`/`STAT`/...
//!   0xFF40-0xFF4B minus the OAM DMA register at 0xFF46, handled
//!   separately below), the APU register block (`NR10`-`NR52`,
//!   0xFF10-0xFF26, plus wave RAM at 0xFF30-0xFF3F), and VRAM
//!   (0x8000-0x9FFF) + OAM (0xFE00-0xFE9F) — routed to a real [`Serial`],
//!   [`Timer`], [`Joypad`], [`crate::apu::Apu`], and [`Ppu`] respectively.
//! - ROM (0x0000-0x7FFF) and external/cartridge RAM (0xA000-0xBFFF) — when
//!   a [`Cartridge`] is installed via [`Mmu::load_cartridge`], routed
//!   there for real bank switching; otherwise (no cartridge — e.g. the
//!   Blargg harness and CPU-core unit tests using [`Mmu::load_rom`]) these
//!   ranges keep falling through to flat memory, unbanked. The two loading
//!   paths are intentionally separate: `load_rom` is fed tiny synthetic
//!   byte sequences in many existing tests that wouldn't survive being
//!   parsed as a cartridge header.
//! - `DMA` (0xFF46) starts an OAM DMA transfer: 160 bytes copied from
//!   `val * 0x100` into OAM over 160 M-cycles (640 T-cycles), one byte
//!   every 4 T-cycles, paced by [`Mmu::step_dma`]. While active, the CPU
//!   (i.e. anything going through the [`Bus`] impl below) can only reach
//!   HRAM (0xFF80-0xFFFF); every other address reads `0xFF` and ignores
//!   writes, matching real hardware's bus conflict. The DMA engine itself
//!   reads through [`Mmu::dispatch_read`] directly, bypassing that gate.
//!
//! I/O-mapped components naturally live behind the bus that owns their
//! address range — the MMU, not `System`, is where this wiring belongs.
//! `IF` (0xFF0F) is likewise real: interrupt dispatch, the
//! serial-complete/timer-overflow/joypad requests all need a live IF
//! byte, and it's just flat memory otherwise.
//!
//! Implements [`crate::cpu::Bus`] so the CPU can drive it directly.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cpu::{Bus, IF_ADDR};
use crate::joypad::{Joypad, JOYPAD_INT_BIT};
use crate::ppu::{Ppu, OAM_BASE, STAT_INT_BIT, VBLANK_INT_BIT, VRAM_BASE};
use crate::serial::{Serial, SERIAL_INT_BIT};
use crate::timer::{Timer, TIMER_INT_BIT};

const JOYP_ADDR: u16 = 0xFF00;
const SB_ADDR: u16 = 0xFF01;
const SC_ADDR: u16 = 0xFF02;
const DIV_ADDR: u16 = 0xFF04;
const TIMA_ADDR: u16 = 0xFF05;
const TMA_ADDR: u16 = 0xFF06;
const TAC_ADDR: u16 = 0xFF07;
const NR10_ADDR: u16 = 0xFF10;
const NR11_ADDR: u16 = 0xFF11;
const NR12_ADDR: u16 = 0xFF12;
const NR13_ADDR: u16 = 0xFF13;
const NR14_ADDR: u16 = 0xFF14;
const NR21_ADDR: u16 = 0xFF16;
const NR22_ADDR: u16 = 0xFF17;
const NR23_ADDR: u16 = 0xFF18;
const NR24_ADDR: u16 = 0xFF19;
const NR30_ADDR: u16 = 0xFF1A;
const NR31_ADDR: u16 = 0xFF1B;
const NR32_ADDR: u16 = 0xFF1C;
const NR33_ADDR: u16 = 0xFF1D;
const NR34_ADDR: u16 = 0xFF1E;
const NR41_ADDR: u16 = 0xFF20;
const NR42_ADDR: u16 = 0xFF21;
const NR43_ADDR: u16 = 0xFF22;
const NR44_ADDR: u16 = 0xFF23;
const NR50_ADDR: u16 = 0xFF24;
const NR51_ADDR: u16 = 0xFF25;
const NR52_ADDR: u16 = 0xFF26;
const WAVE_RAM_START: u16 = 0xFF30;
const WAVE_RAM_END: u16 = 0xFF3F;
const LCDC_ADDR: u16 = 0xFF40;
const STAT_ADDR: u16 = 0xFF41;
const SCY_ADDR: u16 = 0xFF42;
const SCX_ADDR: u16 = 0xFF43;
const LY_ADDR: u16 = 0xFF44;
const LYC_ADDR: u16 = 0xFF45;
const DMA_ADDR: u16 = 0xFF46;
const BGP_ADDR: u16 = 0xFF47;
const OBP0_ADDR: u16 = 0xFF48;
const OBP1_ADDR: u16 = 0xFF49;
const WY_ADDR: u16 = 0xFF4A;
const WX_ADDR: u16 = 0xFF4B;
const VRAM_END: u16 = 0x9FFF;
const OAM_END: u16 = 0xFE9F;
const ROM_START: u16 = 0x0000;
const ROM_END: u16 = 0x7FFF;
const CART_RAM_START: u16 = 0xA000;
const CART_RAM_END: u16 = 0xBFFF;
/// Accessible to the CPU even while an OAM DMA transfer is in progress.
const HRAM_START: u16 = 0xFF80;
const HRAM_END: u16 = 0xFFFF;

const DMA_TRANSFER_BYTES: u16 = 160;
const DMA_T_CYCLES_PER_BYTE: u32 = 4;

/// Flat 64KB-addressable memory, plus the real serial port, timer, PPU
/// registers, joypad, and (once loaded) cartridge. Every other address
/// hits the same backing array for every address — no region-specific
/// behavior.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Mmu {
    // Boxed (heap-allocated) rather than a bare [u8; 0x10000]: besides
    // avoiding a 64KB stack copy on every Mmu move, it lets this field
    // serialize as a plain byte sequence (Box<[u8]>'s native serde impl)
    // instead of needing serde_big_array's fixed-size-array workaround --
    // which, combined with the PPU's similarly large arrays, was blowing
    // the stack during save-state deserialization on threads with a
    // constrained stack size (e.g. cargo test's default worker threads).
    mem: Box<[u8]>,
    pub serial: Serial,
    pub timer: Timer,
    pub ppu: Ppu,
    pub joypad: Joypad,
    pub apu: Apu,
    pub cartridge: Option<Cartridge>,

    dma_active: bool,
    dma_source_high: u8,
    dma_progress: u16,
    dma_cycle_accum: u32,
}

impl std::fmt::Debug for Mmu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mmu")
            .field("mem", &"[u8; 65536]")
            .field("serial", &self.serial)
            .field("timer", &self.timer)
            .field("ppu", &self.ppu)
            .field("joypad", &self.joypad)
            .field("apu", &self.apu)
            .field("cartridge", &self.cartridge)
            .field("dma_active", &self.dma_active)
            .finish()
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self {
            mem: vec![0u8; 0x10000].into_boxed_slice(),
            serial: Serial::new(),
            timer: Timer::new(),
            ppu: Ppu::new(),
            joypad: Joypad::new(),
            apu: Apu::new(),
            cartridge: None,
            dma_active: false,
            dma_source_high: 0,
            dma_progress: 0,
            dma_cycle_accum: 0,
        }
    }
}

impl Mmu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load ROM bytes starting at address 0x0000, truncated to fit the
    /// address space. No banking, and clears any installed cartridge —
    /// this is the flat-memory loading path used by the Blargg harness
    /// and CPU-core unit tests that feed it small synthetic byte
    /// sequences no real cartridge header. See [`Mmu::load_cartridge`]
    /// for real ROM files.
    pub fn load_rom(&mut self, data: &[u8]) {
        self.cartridge = None;
        let len = data.len().min(self.mem.len());
        self.mem[..len].copy_from_slice(&data[..len]);
    }

    /// Parses `data` as a real cartridge image (header, MBC, banking —
    /// see `cartridge/mod.rs`) and installs it, replacing any previous
    /// cartridge or flat-loaded ROM. Returns non-fatal validation
    /// warnings from header parsing.
    pub fn load_cartridge(&mut self, data: &[u8]) -> Vec<String> {
        let (cartridge, warnings) = Cartridge::from_rom_bytes(data);
        self.cartridge = Some(cartridge);
        warnings
    }

    /// Advances the timer by `t_cycles` T-cycles and folds a resulting
    /// overflow into `IF`. Called from `System::step` once per CPU
    /// instruction with its elapsed cycle count.
    pub fn step_timer(&mut self, t_cycles: u8) {
        self.timer.step(t_cycles);
        if self.timer.take_interrupt() {
            self.mem[IF_ADDR as usize] |= TIMER_INT_BIT;
        }
    }

    /// Advances the PPU mode sequencer by `t_cycles` T-cycles and folds any
    /// resulting VBlank/STAT interrupts into `IF`. Called from
    /// `System::step` once per CPU instruction with its elapsed cycle
    /// count.
    pub fn step_ppu(&mut self, t_cycles: u8) {
        self.ppu.step(t_cycles);
        if self.ppu.take_vblank_interrupt() {
            self.mem[IF_ADDR as usize] |= VBLANK_INT_BIT;
        }
        if self.ppu.take_stat_interrupt() {
            self.mem[IF_ADDR as usize] |= STAT_INT_BIT;
        }
    }

    /// Advances the installed cartridge's MBC3 RTC (no-op for every other
    /// MBC, or no cartridge). Called from `System::step` alongside the
    /// timer/PPU.
    pub fn step_cartridge(&mut self, t_cycles: u8) {
        if let Some(cart) = &mut self.cartridge {
            cart.step(t_cycles);
        }
    }

    /// Folds any joypad interrupt request (raised the instant a button
    /// state changes, from `self.joypad.set_button`/a `JOYP` write) into
    /// `IF`. Called from `System::step` once per CPU instruction.
    pub fn step_joypad(&mut self) {
        if self.joypad.take_interrupt() {
            self.mem[IF_ADDR as usize] |= JOYPAD_INT_BIT;
        }
    }

    /// Advances the APU (all 4 channels, frame sequencer, and sample
    /// generation into its ring buffer) by `t_cycles` T-cycles. Called
    /// from `System::step` once per CPU instruction.
    pub fn step_apu(&mut self, t_cycles: u8) {
        self.apu.step(t_cycles);
    }

    /// Advances an in-progress OAM DMA transfer by `t_cycles` T-cycles,
    /// copying one byte every 4 T-cycles until all 160 are done. No-op
    /// when no transfer is active. Called from `System::step` once per CPU
    /// instruction.
    pub fn step_dma(&mut self, t_cycles: u8) {
        if !self.dma_active {
            return;
        }
        self.dma_cycle_accum += t_cycles as u32;
        while self.dma_active && self.dma_cycle_accum >= DMA_T_CYCLES_PER_BYTE {
            self.dma_cycle_accum -= DMA_T_CYCLES_PER_BYTE;
            let src = ((self.dma_source_high as u16) << 8) + self.dma_progress;
            let byte = self.dispatch_read(src);
            self.ppu.write_oam(OAM_BASE + self.dma_progress, byte);
            self.dma_progress += 1;
            if self.dma_progress >= DMA_TRANSFER_BYTES {
                self.dma_active = false;
            }
        }
    }

    /// The actual per-address read dispatch, shared by the gated
    /// `Bus::read` (the CPU's view) and the DMA engine (which has full bus
    /// access regardless of an in-progress transfer).
    fn dispatch_read(&self, addr: u16) -> u8 {
        match addr {
            JOYP_ADDR => self.joypad.read_joyp(),
            SB_ADDR => self.serial.read_sb(),
            SC_ADDR => self.serial.read_sc(),
            DIV_ADDR => self.timer.read_div(),
            TIMA_ADDR => self.timer.read_tima(),
            TMA_ADDR => self.timer.read_tma(),
            TAC_ADDR => self.timer.read_tac(),
            NR10_ADDR => self.apu.read_nr10(),
            NR11_ADDR => self.apu.read_nr11(),
            NR12_ADDR => self.apu.read_nr12(),
            NR13_ADDR | NR23_ADDR | NR31_ADDR | NR33_ADDR | NR41_ADDR => 0xFF, // write-only
            NR14_ADDR => self.apu.read_nr14(),
            NR21_ADDR => self.apu.read_nr21(),
            NR22_ADDR => self.apu.read_nr22(),
            NR24_ADDR => self.apu.read_nr24(),
            NR30_ADDR => self.apu.read_nr30(),
            NR32_ADDR => self.apu.read_nr32(),
            NR34_ADDR => self.apu.read_nr34(),
            NR42_ADDR => self.apu.read_nr42(),
            NR43_ADDR => self.apu.read_nr43(),
            NR44_ADDR => self.apu.read_nr44(),
            NR50_ADDR => self.apu.read_nr50(),
            NR51_ADDR => self.apu.read_nr51(),
            NR52_ADDR => self.apu.read_nr52(),
            WAVE_RAM_START..=WAVE_RAM_END => self.apu.read_wave_ram(addr),
            LCDC_ADDR => self.ppu.read_lcdc(),
            STAT_ADDR => self.ppu.read_stat(),
            SCY_ADDR => self.ppu.read_scy(),
            SCX_ADDR => self.ppu.read_scx(),
            LY_ADDR => self.ppu.read_ly(),
            LYC_ADDR => self.ppu.read_lyc(),
            DMA_ADDR => self.dma_source_high,
            BGP_ADDR => self.ppu.read_bgp(),
            OBP0_ADDR => self.ppu.read_obp0(),
            OBP1_ADDR => self.ppu.read_obp1(),
            WY_ADDR => self.ppu.read_wy(),
            WX_ADDR => self.ppu.read_wx(),
            VRAM_BASE..=VRAM_END => self.ppu.read_vram(addr),
            OAM_BASE..=OAM_END => self.ppu.read_oam(addr),
            ROM_START..=ROM_END => match &self.cartridge {
                Some(cart) => cart.read_rom(addr),
                None => self.mem[addr as usize],
            },
            CART_RAM_START..=CART_RAM_END => match &self.cartridge {
                Some(cart) => cart.read_ram(addr),
                None => self.mem[addr as usize],
            },
            _ => self.mem[addr as usize],
        }
    }

    /// The actual per-address write dispatch; see [`Mmu::dispatch_read`].
    fn dispatch_write(&mut self, addr: u16, val: u8) {
        match addr {
            JOYP_ADDR => self.joypad.write_joyp(val),
            SB_ADDR => self.serial.write_sb(val),
            SC_ADDR => {
                self.serial.write_sc(val);
                if self.serial.take_interrupt() {
                    self.mem[IF_ADDR as usize] |= SERIAL_INT_BIT;
                }
            }
            DIV_ADDR => self.timer.write_div(val),
            TIMA_ADDR => self.timer.write_tima(val),
            TMA_ADDR => self.timer.write_tma(val),
            TAC_ADDR => self.timer.write_tac(val),
            NR10_ADDR => self.apu.write_nr10(val),
            NR11_ADDR => self.apu.write_nr11(val),
            NR12_ADDR => self.apu.write_nr12(val),
            NR13_ADDR => self.apu.write_nr13(val),
            NR14_ADDR => self.apu.write_nr14(val),
            NR21_ADDR => self.apu.write_nr21(val),
            NR22_ADDR => self.apu.write_nr22(val),
            NR23_ADDR => self.apu.write_nr23(val),
            NR24_ADDR => self.apu.write_nr24(val),
            NR30_ADDR => self.apu.write_nr30(val),
            NR31_ADDR => self.apu.write_nr31(val),
            NR32_ADDR => self.apu.write_nr32(val),
            NR33_ADDR => self.apu.write_nr33(val),
            NR34_ADDR => self.apu.write_nr34(val),
            NR41_ADDR => self.apu.write_nr41(val),
            NR42_ADDR => self.apu.write_nr42(val),
            NR43_ADDR => self.apu.write_nr43(val),
            NR44_ADDR => self.apu.write_nr44(val),
            NR50_ADDR => self.apu.write_nr50(val),
            NR51_ADDR => self.apu.write_nr51(val),
            NR52_ADDR => self.apu.write_nr52(val),
            WAVE_RAM_START..=WAVE_RAM_END => self.apu.write_wave_ram(addr, val),
            LCDC_ADDR => self.ppu.write_lcdc(val),
            STAT_ADDR => self.ppu.write_stat(val),
            SCY_ADDR => self.ppu.write_scy(val),
            SCX_ADDR => self.ppu.write_scx(val),
            LY_ADDR => self.ppu.write_ly(val),
            LYC_ADDR => self.ppu.write_lyc(val),
            DMA_ADDR => {
                self.dma_source_high = val;
                self.dma_progress = 0;
                self.dma_cycle_accum = 0;
                self.dma_active = true;
            }
            BGP_ADDR => self.ppu.write_bgp(val),
            OBP0_ADDR => self.ppu.write_obp0(val),
            OBP1_ADDR => self.ppu.write_obp1(val),
            WY_ADDR => self.ppu.write_wy(val),
            WX_ADDR => self.ppu.write_wx(val),
            VRAM_BASE..=VRAM_END => self.ppu.write_vram(addr, val),
            OAM_BASE..=OAM_END => self.ppu.write_oam(addr, val),
            ROM_START..=ROM_END => {
                if let Some(cart) = &mut self.cartridge {
                    cart.write_rom_register(addr, val);
                } else {
                    self.mem[addr as usize] = val;
                }
            }
            CART_RAM_START..=CART_RAM_END => {
                if let Some(cart) = &mut self.cartridge {
                    cart.write_ram(addr, val);
                } else {
                    self.mem[addr as usize] = val;
                }
            }
            _ => self.mem[addr as usize] = val,
        }
    }
}

impl Bus for Mmu {
    fn read(&mut self, addr: u16) -> u8 {
        if self.dma_active && !(HRAM_START..=HRAM_END).contains(&addr) {
            return 0xFF;
        }
        self.dispatch_read(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        if self.dma_active && !(HRAM_START..=HRAM_END).contains(&addr) {
            return;
        }
        self.dispatch_write(addr, val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_round_trip_across_full_address_space() {
        let mut mmu = Mmu::new();
        mmu.write(0x0000, 0x11);
        mmu.write(0x7FFF, 0x22);
        mmu.write(0x8000, 0x33);
        mmu.write(0xFFFF, 0x44);
        assert_eq!(mmu.read(0x0000), 0x11);
        assert_eq!(mmu.read(0x7FFF), 0x22);
        assert_eq!(mmu.read(0x8000), 0x33);
        assert_eq!(mmu.read(0xFFFF), 0x44);
    }

    #[test]
    fn load_rom_copies_bytes_from_zero() {
        let mut mmu = Mmu::new();
        mmu.load_rom(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(mmu.read(0x0000), 0xAA);
        assert_eq!(mmu.read(0x0001), 0xBB);
        assert_eq!(mmu.read(0x0002), 0xCC);
        assert_eq!(mmu.read(0x0003), 0x00);
    }

    #[test]
    fn load_rom_truncates_oversized_data() {
        let mut mmu = Mmu::new();
        let data = vec![0x42u8; 0x20000]; // bigger than 64KB
        mmu.load_rom(&data);
        assert_eq!(mmu.read(0xFFFF), 0x42);
    }

    #[test]
    fn ppu_registers_are_routed_to_the_ppu_not_flat_memory() {
        let mut mmu = Mmu::new();
        mmu.write(LCDC_ADDR, 0x91);
        mmu.write(SCY_ADDR, 7);
        mmu.write(BGP_ADDR, 0xE4);
        assert_eq!(mmu.read(LCDC_ADDR), 0x91);
        assert_eq!(mmu.read(SCY_ADDR), 7);
        assert_eq!(mmu.read(BGP_ADDR), 0xE4);
        // LY is read-only; writes through the bus are ignored.
        mmu.write(LY_ADDR, 42);
        assert_eq!(mmu.read(LY_ADDR), 0);
    }

    #[test]
    fn load_cartridge_routes_rom_and_ram_through_the_cartridge() {
        // Minimal valid MBC1+RAM+BATTERY header: 4 ROM banks (64KB), 1 RAM
        // bank (8KB, code 0x02).
        let mut data = vec![0u8; 4 * 0x4000];
        data[0x0147] = 0x03; // MBC1+RAM+BATTERY
        data[0x0148] = 0x01; // 4 banks
        data[0x0149] = 0x02; // 8KB RAM
        data[3 * 0x4000] = 0xAB; // bank 3, offset 0
        let checksum = (0x0134..0x014D)
            .map(|a| data[a])
            .fold(0u8, |acc, b| acc.wrapping_sub(b).wrapping_sub(1));
        data[0x014D] = checksum;

        let mut mmu = Mmu::new();
        let warnings = mmu.load_cartridge(&data);
        assert!(warnings.is_empty());

        mmu.write(0x2000, 3); // select ROM bank 3
        assert_eq!(mmu.read(0x4000), 0xAB);

        mmu.write(0x0000, 0x0A); // enable RAM
        mmu.write(0xA000, 0x77);
        assert_eq!(mmu.read(0xA000), 0x77);
    }

    #[test]
    fn cpu_can_execute_through_mmu_bus() {
        use crate::cpu::Cpu;
        let mut mmu = Mmu::new();
        mmu.load_rom(&[0x3E, 0x2A]); // LD A, 0x2A
        let mut cpu = Cpu::new();
        let t = cpu.step(&mut mmu);
        assert_eq!(cpu.regs.a, 0x2A);
        assert_eq!(t, 8);
    }

    #[test]
    fn joyp_is_routed_to_the_joypad_not_flat_memory() {
        use crate::joypad::Button;
        let mut mmu = Mmu::new();
        mmu.write(JOYP_ADDR, 0x20); // select direction row
        mmu.joypad.set_button(Button::Up, true);
        assert_eq!(mmu.read(JOYP_ADDR) & 0x0F, 0b1011); // bit 2 (Up) low
    }

    #[test]
    fn apu_registers_and_wave_ram_are_routed_to_the_apu() {
        let mut mmu = Mmu::new();
        mmu.write(NR52_ADDR, 0x80); // power on
        mmu.write(NR50_ADDR, 0x77);
        mmu.write(NR51_ADDR, 0xFF);
        mmu.write(WAVE_RAM_START, 0xAB);
        assert_eq!(mmu.read(NR50_ADDR), 0x77);
        assert_eq!(mmu.read(NR51_ADDR), 0xFF);
        assert_eq!(mmu.read(WAVE_RAM_START), 0xAB);
        // Write-only registers read back as open bus.
        mmu.write(NR13_ADDR, 0x42);
        assert_eq!(mmu.read(NR13_ADDR), 0xFF);
    }

    #[test]
    fn oam_dma_copies_160_bytes_over_640_t_cycles() {
        let mut mmu = Mmu::new();
        for i in 0..DMA_TRANSFER_BYTES {
            mmu.mem[0xC000 + i as usize] = i as u8;
        }
        mmu.write(DMA_ADDR, 0xC0); // source = 0xC000

        // Not done yet partway through (600 of 640 T-cycles).
        mmu.step_dma(255);
        mmu.step_dma(255);
        mmu.step_dma(90);
        assert_ne!(mmu.ppu.read_oam(OAM_BASE + 159), 159);

        mmu.step_dma(40); // completes the remaining 40 T-cycles (10 bytes)
        for i in 0..DMA_TRANSFER_BYTES {
            assert_eq!(mmu.ppu.read_oam(OAM_BASE + i), i as u8);
        }
    }

    #[test]
    fn oam_dma_blocks_cpu_access_to_everything_but_hram() {
        let mut mmu = Mmu::new();
        mmu.write(0xC000, 0x11); // before DMA: ordinary WRAM write works
        mmu.write(DMA_ADDR, 0xC0);

        mmu.write(0xC000, 0x22); // blocked: not HRAM
        assert_eq!(mmu.read(0xC000), 0xFF); // reads as open bus too
        mmu.write(0xFF80, 0x33); // HRAM stays accessible
        assert_eq!(mmu.read(0xFF80), 0x33);

        mmu.step_dma(255);
        mmu.step_dma(255);
        mmu.step_dma(130); // finish the transfer (640 T-cycles total)
        assert_eq!(mmu.read(0xC000), 0x11); // unblocked; the 0x22 write never landed
    }
}
