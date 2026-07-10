//! Memory management unit: bus, memory map, I/O register dispatch.
//!
//! The general address space is still a flat 64KB byte array for anything
//! not routed elsewhere (WRAM/HRAM/echo RAM — no real behavioral
//! difference from flat memory yet). Real routing exists for the regions
//! that need actual side effects or backing storage beyond "a byte":
//!
//! - `SB`/`SC` (0xFF01/0xFF02), `DIV`/`TIMA`/`TMA`/`TAC` (0xFF04-0xFF07),
//!   the PPU register block (`LCDC`/`STAT`/... 0xFF40-0xFF4B minus the OAM
//!   DMA register at 0xFF46, which lands with M4 DMA timing), and VRAM
//!   (0x8000-0x9FFF) + OAM (0xFE00-0xFE9F) — routed to a real [`Serial`],
//!   [`Timer`], and [`Ppu`] respectively.
//! - ROM (0x0000-0x7FFF) and external/cartridge RAM (0xA000-0xBFFF) — when
//!   a [`Cartridge`] is installed via [`Mmu::load_cartridge`], routed
//!   there for real bank switching; otherwise (no cartridge — e.g. the
//!   Blargg harness and CPU-core unit tests using [`Mmu::load_rom`]) these
//!   ranges keep falling through to flat memory, unbanked. The two loading
//!   paths are intentionally separate: `load_rom` is fed tiny synthetic
//!   byte sequences in many existing tests that wouldn't survive being
//!   parsed as a cartridge header.
//!
//! I/O-mapped components naturally live behind the bus that owns their
//! address range — the MMU, not `System`, is where this and later
//! APU/Joypad register wiring belong. `IF` (0xFF0F) is likewise real:
//! interrupt dispatch, the serial-complete request, and the timer-overflow
//! request all need a live IF byte, and it's just flat memory until other
//! interrupt sources (Joypad) land.
//!
//! Implements [`crate::cpu::Bus`] so the CPU can drive it directly.

use crate::cartridge::Cartridge;
use crate::cpu::{Bus, IF_ADDR};
use crate::ppu::{Ppu, OAM_BASE, STAT_INT_BIT, VBLANK_INT_BIT, VRAM_BASE};
use crate::serial::{Serial, SERIAL_INT_BIT};
use crate::timer::{Timer, TIMER_INT_BIT};

const SB_ADDR: u16 = 0xFF01;
const SC_ADDR: u16 = 0xFF02;
const DIV_ADDR: u16 = 0xFF04;
const TIMA_ADDR: u16 = 0xFF05;
const TMA_ADDR: u16 = 0xFF06;
const TAC_ADDR: u16 = 0xFF07;
const LCDC_ADDR: u16 = 0xFF40;
const STAT_ADDR: u16 = 0xFF41;
const SCY_ADDR: u16 = 0xFF42;
const SCX_ADDR: u16 = 0xFF43;
const LY_ADDR: u16 = 0xFF44;
const LYC_ADDR: u16 = 0xFF45;
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

/// Flat 64KB-addressable memory, plus the real serial port, timer, PPU
/// registers, and (once loaded) cartridge. Every other address hits the
/// same backing array for every address — no region-specific behavior.
#[derive(Clone)]
pub struct Mmu {
    mem: [u8; 0x10000],
    pub serial: Serial,
    pub timer: Timer,
    pub ppu: Ppu,
    pub cartridge: Option<Cartridge>,
}

impl std::fmt::Debug for Mmu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mmu")
            .field("mem", &"[u8; 65536]")
            .field("serial", &self.serial)
            .field("timer", &self.timer)
            .field("ppu", &self.ppu)
            .field("cartridge", &self.cartridge)
            .finish()
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self {
            mem: [0; 0x10000],
            serial: Serial::new(),
            timer: Timer::new(),
            ppu: Ppu::new(),
            cartridge: None,
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
}

impl Bus for Mmu {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            SB_ADDR => self.serial.read_sb(),
            SC_ADDR => self.serial.read_sc(),
            DIV_ADDR => self.timer.read_div(),
            TIMA_ADDR => self.timer.read_tima(),
            TMA_ADDR => self.timer.read_tma(),
            TAC_ADDR => self.timer.read_tac(),
            LCDC_ADDR => self.ppu.read_lcdc(),
            STAT_ADDR => self.ppu.read_stat(),
            SCY_ADDR => self.ppu.read_scy(),
            SCX_ADDR => self.ppu.read_scx(),
            LY_ADDR => self.ppu.read_ly(),
            LYC_ADDR => self.ppu.read_lyc(),
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

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
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
            LCDC_ADDR => self.ppu.write_lcdc(val),
            STAT_ADDR => self.ppu.write_stat(val),
            SCY_ADDR => self.ppu.write_scy(val),
            SCX_ADDR => self.ppu.write_scx(val),
            LY_ADDR => self.ppu.write_ly(val),
            LYC_ADDR => self.ppu.write_lyc(val),
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
}
