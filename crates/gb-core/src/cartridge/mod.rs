//! Cartridge loading: header parsing + validation, and MBC0/1/2/3/5 bank
//! switching.
//!
//! `Cartridge::from_rom_bytes` parses the header at 0x0100-0x014F, picks
//! the MBC implied by the cart type byte, and returns both the cartridge
//! and a list of non-fatal validation warnings (bad checksum, ROM/RAM size
//! mismatch, unsupported cart type — the header is trusted and loading
//! proceeds regardless, matching real hardware's "just try to run it"
//! behavior). `Mmu` routes 0x0000-0x7FFF (ROM + MBC control registers) and
//! 0xA000-0xBFFF (external/cartridge RAM) here when a cartridge is
//! installed; `System::load_rom`'s flat unbanked load (used by the
//! existing Blargg harness and CPU-core unit tests, several of which feed
//! it tiny non-cartridge byte sequences that wouldn't survive header
//! parsing) is untouched and takes priority when no cartridge is
//! installed — see `mmu/mod.rs`.
//!
//! Battery-backed RAM persistence is a save/load of raw bytes
//! ([`Cartridge::battery_ram`]/[`Cartridge::load_battery_ram`]) plus a
//! dirty flag ([`Cartridge::is_battery_dirty`]); actual `.sav` file I/O is
//! `gb-tui`'s job (on load, on exit, and on a dirty-flag interval), not
//! `gb-core`'s — same reasoning as the terminal/audio boundary elsewhere
//! in this crate.

const ROM_BANK_SIZE: usize = 0x4000;
const RAM_BANK_SIZE: usize = 0x2000;
/// MBC2's RAM is 512 x 4-bit nibbles built into the MBC chip itself, not a
/// separately sized external RAM chip.
const MBC2_RAM_NIBBLES: usize = 512;

const TITLE_START: usize = 0x0134;
const TITLE_END: usize = 0x0144; // exclusive
const CART_TYPE_ADDR: usize = 0x0147;
const ROM_SIZE_ADDR: usize = 0x0148;
const RAM_SIZE_ADDR: usize = 0x0149;
const HEADER_CHECKSUM_ADDR: usize = 0x014D;
const HEADER_CHECKSUM_START: usize = 0x0134;
const HEADER_CHECKSUM_END: usize = 0x014D; // exclusive
const HEADER_MIN_LEN: usize = 0x0150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbcType {
    None,
    Mbc1,
    Mbc2,
    Mbc3,
    Mbc5,
    /// Cart type byte doesn't map to a supported MBC. Treated like `None`
    /// (fixed 32KB, no RAM) so loading still proceeds; a warning is
    /// returned from `from_rom_bytes`.
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct Header {
    pub title: String,
    pub cart_type: u8,
    pub mbc: MbcType,
    pub rom_banks: usize,
    pub ram_banks: usize,
    pub has_ram: bool,
    pub has_battery: bool,
    pub has_rtc: bool,
    pub has_rumble: bool,
    pub header_checksum_valid: bool,
}

/// Cart-type-byte -> (MBC, has_ram, has_battery, has_rtc, has_rumble).
/// MBC2 always has RAM (its built-in nibble RAM) regardless of the RAM
/// size byte, which is why it's listed explicitly rather than relying on
/// `ram_size_code`.
fn mbc_traits(cart_type: u8) -> (MbcType, bool, bool, bool, bool) {
    use MbcType::*;
    match cart_type {
        0x00 => (None, false, false, false, false),
        0x01 => (Mbc1, false, false, false, false),
        0x02 => (Mbc1, true, false, false, false),
        0x03 => (Mbc1, true, true, false, false),
        0x05 => (Mbc2, true, false, false, false),
        0x06 => (Mbc2, true, true, false, false),
        0x08 => (None, true, false, false, false),
        0x09 => (None, true, true, false, false),
        0x0F => (Mbc3, false, true, true, false),
        0x10 => (Mbc3, true, true, true, false),
        0x11 => (Mbc3, false, false, false, false),
        0x12 => (Mbc3, true, false, false, false),
        0x13 => (Mbc3, true, true, false, false),
        0x19 => (Mbc5, false, false, false, false),
        0x1A => (Mbc5, true, false, false, false),
        0x1B => (Mbc5, true, true, false, false),
        0x1C => (Mbc5, false, false, false, true),
        0x1D => (Mbc5, true, false, false, true),
        0x1E => (Mbc5, true, true, false, true),
        _ => (Unsupported, false, false, false, false),
    }
}

fn rom_banks_for_code(code: u8) -> Option<usize> {
    // 0x00-0x08: 2 << code banks (32KB * 2^code). Larger/irregular codes
    // (0x52/0x53/0x54 for 1.1/1.2/1.5MB carts) aren't handled — rare
    // enough that a warning + best-effort fallback to the data length is
    // the pragmatic choice.
    if code <= 0x08 {
        Some(2usize << code)
    } else {
        None
    }
}

fn ram_banks_for_code(code: u8) -> Option<usize> {
    match code {
        0x00 => Some(0),
        0x01 => Some(1), // 2KB, unofficial/rare
        0x02 => Some(1), // 8KB
        0x03 => Some(4), // 32KB
        0x04 => Some(16), // 128KB
        0x05 => Some(8), // 64KB
        _ => None,
    }
}

impl Header {
    fn parse(data: &[u8], warnings: &mut Vec<String>) -> Self {
        let byte = |addr: usize| -> u8 { data.get(addr).copied().unwrap_or(0) };

        let title_bytes: Vec<u8> = (TITLE_START..TITLE_END)
            .map(byte)
            .take_while(|&b| b != 0)
            .collect();
        let title = String::from_utf8_lossy(&title_bytes).into_owned();

        let cart_type = byte(CART_TYPE_ADDR);
        let (mbc, has_ram, has_battery, has_rtc, has_rumble) = mbc_traits(cart_type);
        if mbc == MbcType::Unsupported {
            warnings.push(format!("unsupported cart type byte 0x{cart_type:02X}; treating as ROM-only"));
        }

        let rom_size_code = byte(ROM_SIZE_ADDR);
        let rom_banks = rom_banks_for_code(rom_size_code).unwrap_or_else(|| {
            warnings.push(format!("unrecognized ROM size code 0x{rom_size_code:02X}"));
            (data.len().div_ceil(ROM_BANK_SIZE)).max(2)
        });
        let expected_len = rom_banks * ROM_BANK_SIZE;
        if data.len() != expected_len && data.len() > HEADER_MIN_LEN {
            warnings.push(format!(
                "ROM size header says {expected_len} bytes but the file is {} bytes",
                data.len()
            ));
        }

        let ram_banks = if mbc == MbcType::Mbc2 {
            0 // sized separately via MBC2_RAM_NIBBLES, not the RAM size byte
        } else if has_ram {
            let ram_size_code = byte(RAM_SIZE_ADDR);
            ram_banks_for_code(ram_size_code).unwrap_or_else(|| {
                warnings.push(format!("unrecognized RAM size code 0x{ram_size_code:02X}"));
                1
            })
        } else {
            0
        };

        let stored_checksum = byte(HEADER_CHECKSUM_ADDR);
        let computed_checksum = (HEADER_CHECKSUM_START..HEADER_CHECKSUM_END)
            .map(byte)
            .fold(0u8, |acc, b| acc.wrapping_sub(b).wrapping_sub(1));
        let header_checksum_valid = stored_checksum == computed_checksum;
        if !header_checksum_valid {
            warnings.push(format!(
                "header checksum mismatch: stored 0x{stored_checksum:02X}, computed 0x{computed_checksum:02X}"
            ));
        }

        Header {
            title,
            cart_type,
            mbc,
            rom_banks,
            ram_banks,
            has_ram,
            has_battery,
            has_rtc,
            has_rumble,
            header_checksum_valid,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Mbc1State {
    ram_enable: bool,
    rom_bank_low5: u8,
    bank2: u8,
    mode1: bool,
}

#[derive(Debug, Clone, Default)]
struct Mbc2State {
    ram_enable: bool,
    rom_bank: u8,
}

#[derive(Debug, Clone, Default)]
struct RtcRegisters {
    seconds: u8,
    minutes: u8,
    hours: u8,
    day_low: u8,
    /// Bit 0: day counter bit 8. Bit 6: halt. Bit 7: day counter carry.
    day_high: u8,
}

#[derive(Debug, Clone, Default)]
struct Mbc3State {
    ram_enable: bool,
    rom_bank: u8,
    /// 0x00-0x03 selects a RAM bank; 0x08-0x0C selects an RTC register.
    ram_bank_or_rtc_select: u8,
    latch_prev_write: u8,
    rtc: RtcRegisters,
    rtc_latched: RtcRegisters,
    /// T-cycles accumulated toward the next RTC second, only while the
    /// RTC isn't halted.
    rtc_cycle_accum: u32,
}

#[derive(Debug, Clone, Default)]
struct Mbc5State {
    ram_enable: bool,
    rom_bank_low8: u8,
    rom_bank_bit8: u8,
    ram_bank: u8,
}

#[derive(Debug, Clone)]
enum MbcState {
    None,
    Mbc1(Mbc1State),
    Mbc2(Mbc2State),
    Mbc3(Mbc3State),
    Mbc5(Mbc5State),
}

/// DMG CPU clock, T-cycles/second — used to pace the MBC3 RTC off elapsed
/// emulated cycles rather than wall-clock time, keeping it deterministic
/// and headless-testable.
const CYCLES_PER_SECOND: u32 = 4_194_304;

#[derive(Debug, Clone)]
pub struct Cartridge {
    pub header: Header,
    rom: Vec<u8>,
    ram: Vec<u8>,
    mbc: MbcState,
    battery_dirty: bool,
}

impl Cartridge {
    /// Parses `data` as a cartridge image and returns it alongside any
    /// non-fatal validation warnings. Never fails — an unrecognized or
    /// malformed header degrades to a best-effort ROM-only cartridge, same
    /// as real hardware just attempting to run whatever's on the cart.
    pub fn from_rom_bytes(data: &[u8]) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let header = Header::parse(data, &mut warnings);

        let mut rom = data.to_vec();
        rom.resize(header.rom_banks * ROM_BANK_SIZE, 0xFF);

        let ram_len = if header.mbc == MbcType::Mbc2 {
            MBC2_RAM_NIBBLES
        } else {
            header.ram_banks * RAM_BANK_SIZE
        };
        let ram = vec![0u8; ram_len];

        let mbc = match header.mbc {
            MbcType::None | MbcType::Unsupported => MbcState::None,
            MbcType::Mbc1 => MbcState::Mbc1(Mbc1State::default()),
            MbcType::Mbc2 => MbcState::Mbc2(Mbc2State::default()),
            MbcType::Mbc3 => MbcState::Mbc3(Mbc3State::default()),
            MbcType::Mbc5 => MbcState::Mbc5(Mbc5State::default()),
        };

        (
            Cartridge { header, rom, ram, mbc, battery_dirty: false },
            warnings,
        )
    }

    pub fn has_battery(&self) -> bool {
        self.header.has_battery
    }

    /// Raw battery-backed RAM contents (cartridge RAM, or MBC3's RTC
    /// registers appended after it) for persisting to a `.sav` file.
    /// Layout: RAM bytes, then (MBC3 with RTC only) 5 latched RTC register
    /// bytes.
    pub fn battery_ram(&self) -> Vec<u8> {
        let mut out = self.ram.clone();
        if let MbcState::Mbc3(s) = &self.mbc {
            if self.header.has_rtc {
                out.extend_from_slice(&[
                    s.rtc.seconds,
                    s.rtc.minutes,
                    s.rtc.hours,
                    s.rtc.day_low,
                    s.rtc.day_high,
                ]);
            }
        }
        out
    }

    /// Restores RAM (and MBC3 RTC registers, if present) from a previously
    /// saved [`Cartridge::battery_ram`] blob. Ignores a length mismatch
    /// beyond what's needed (e.g. loading a save from before RTC support)
    /// rather than failing.
    pub fn load_battery_ram(&mut self, data: &[u8]) {
        let ram_len = self.ram.len();
        let n = data.len().min(ram_len);
        self.ram[..n].copy_from_slice(&data[..n]);
        if let MbcState::Mbc3(s) = &mut self.mbc {
            if self.header.has_rtc && data.len() >= ram_len + 5 {
                let rtc = &data[ram_len..ram_len + 5];
                s.rtc.seconds = rtc[0];
                s.rtc.minutes = rtc[1];
                s.rtc.hours = rtc[2];
                s.rtc.day_low = rtc[3];
                s.rtc.day_high = rtc[4];
            }
        }
        self.battery_dirty = false;
    }

    pub fn is_battery_dirty(&self) -> bool {
        self.battery_dirty
    }

    pub fn clear_battery_dirty(&mut self) {
        self.battery_dirty = false;
    }

    /// Advances MBC3's RTC by `t_cycles` T-cycles (no-op for every other
    /// MBC). Called from `System::step` alongside the timer/PPU.
    pub fn step(&mut self, t_cycles: u8) {
        if let MbcState::Mbc3(s) = &mut self.mbc {
            if s.rtc.day_high & 0x40 != 0 {
                return; // RTC halted
            }
            s.rtc_cycle_accum += t_cycles as u32;
            while s.rtc_cycle_accum >= CYCLES_PER_SECOND {
                s.rtc_cycle_accum -= CYCLES_PER_SECOND;
                tick_rtc_second(&mut s.rtc);
            }
        }
    }

    /// Reads from 0x0000-0x7FFF (fixed bank 0 + switchable bank).
    pub fn read_rom(&self, addr: u16) -> u8 {
        let bank = match &self.mbc {
            MbcState::None => {
                if addr < 0x4000 {
                    0
                } else {
                    1
                }
            }
            MbcState::Mbc1(s) => {
                if addr < 0x4000 {
                    if s.mode1 && self.header.rom_banks > 32 {
                        ((s.bank2 as usize) << 5) & (self.header.rom_banks - 1)
                    } else {
                        0
                    }
                } else {
                    let low5 = if s.rom_bank_low5 == 0 { 1 } else { s.rom_bank_low5 };
                    (((s.bank2 as usize) << 5) | low5 as usize) & (self.header.rom_banks - 1)
                }
            }
            MbcState::Mbc2(s) => {
                if addr < 0x4000 {
                    0
                } else {
                    let bank = if s.rom_bank == 0 { 1 } else { s.rom_bank };
                    (bank as usize) & (self.header.rom_banks - 1)
                }
            }
            MbcState::Mbc3(s) => {
                if addr < 0x4000 {
                    0
                } else {
                    let bank = if s.rom_bank == 0 { 1 } else { s.rom_bank };
                    (bank as usize) & (self.header.rom_banks - 1)
                }
            }
            MbcState::Mbc5(s) => {
                if addr < 0x4000 {
                    0
                } else {
                    let bank = ((s.rom_bank_bit8 as usize) << 8) | s.rom_bank_low8 as usize;
                    bank & (self.header.rom_banks - 1)
                }
            }
        };
        let offset = (addr as usize) % ROM_BANK_SIZE;
        self.rom[bank * ROM_BANK_SIZE + offset]
    }

    /// Writes to 0x0000-0x7FFF: MBC control registers, not actual ROM
    /// content.
    pub fn write_rom_register(&mut self, addr: u16, val: u8) {
        match &mut self.mbc {
            MbcState::None => {}
            MbcState::Mbc1(s) => match addr {
                0x0000..=0x1FFF => s.ram_enable = val & 0x0F == 0x0A,
                0x2000..=0x3FFF => s.rom_bank_low5 = val & 0x1F,
                0x4000..=0x5FFF => s.bank2 = val & 0x03,
                0x6000..=0x7FFF => s.mode1 = val & 0x01 != 0,
                _ => {}
            },
            MbcState::Mbc2(s) => {
                if addr < 0x4000 {
                    if addr & 0x0100 == 0 {
                        s.ram_enable = val & 0x0F == 0x0A;
                    } else {
                        s.rom_bank = val & 0x0F;
                    }
                }
            }
            MbcState::Mbc3(s) => match addr {
                0x0000..=0x1FFF => s.ram_enable = val & 0x0F == 0x0A,
                0x2000..=0x3FFF => s.rom_bank = val & 0x7F,
                0x4000..=0x5FFF => s.ram_bank_or_rtc_select = val,
                0x6000..=0x7FFF => {
                    if s.latch_prev_write == 0x00 && val == 0x01 {
                        s.rtc_latched = s.rtc.clone();
                    }
                    s.latch_prev_write = val;
                }
                _ => {}
            },
            MbcState::Mbc5(s) => match addr {
                0x0000..=0x1FFF => s.ram_enable = val & 0x0F == 0x0A,
                0x2000..=0x2FFF => s.rom_bank_low8 = val,
                0x3000..=0x3FFF => s.rom_bank_bit8 = val & 0x01,
                0x4000..=0x5FFF => s.ram_bank = val & 0x0F,
                _ => {}
            },
        }
    }

    /// Reads from 0xA000-0xBFFF (external/cartridge RAM). Returns open-bus
    /// `0xFF` when RAM is disabled or absent.
    pub fn read_ram(&self, addr: u16) -> u8 {
        match &self.mbc {
            MbcState::None => self.ram_byte_at(0, addr),
            MbcState::Mbc1(s) => {
                if !s.ram_enable || self.ram.is_empty() {
                    return 0xFF;
                }
                let bank = if s.mode1 { s.bank2 as usize } else { 0 };
                self.ram_byte_at(bank, addr)
            }
            MbcState::Mbc2(s) => {
                if !s.ram_enable {
                    return 0xFF;
                }
                0xF0 | self.mbc2_nibble(addr)
            }
            MbcState::Mbc3(s) => {
                if !s.ram_enable {
                    return 0xFF;
                }
                match s.ram_bank_or_rtc_select {
                    0x00..=0x03 => self.ram_byte_at(s.ram_bank_or_rtc_select as usize, addr),
                    0x08 => s.rtc_latched.seconds,
                    0x09 => s.rtc_latched.minutes,
                    0x0A => s.rtc_latched.hours,
                    0x0B => s.rtc_latched.day_low,
                    0x0C => s.rtc_latched.day_high,
                    _ => 0xFF,
                }
            }
            MbcState::Mbc5(s) => {
                if !s.ram_enable || self.ram.is_empty() {
                    return 0xFF;
                }
                self.ram_byte_at(s.ram_bank as usize, addr)
            }
        }
    }

    pub fn write_ram(&mut self, addr: u16, val: u8) {
        match &mut self.mbc {
            MbcState::None => {}
            MbcState::Mbc1(s) => {
                if !s.ram_enable || self.ram.is_empty() {
                    return;
                }
                let bank = if s.mode1 { s.bank2 as usize } else { 0 };
                self.set_ram_byte_at(bank, addr, val);
                self.battery_dirty |= self.header.has_battery;
            }
            MbcState::Mbc2(s) => {
                if !s.ram_enable {
                    return;
                }
                let idx = (addr as usize - 0xA000) % MBC2_RAM_NIBBLES;
                self.ram[idx] = val & 0x0F;
                self.battery_dirty |= self.header.has_battery;
            }
            MbcState::Mbc3(s) => {
                if !s.ram_enable {
                    return;
                }
                match s.ram_bank_or_rtc_select {
                    0x00..=0x03 => {
                        let bank = s.ram_bank_or_rtc_select as usize;
                        self.set_ram_byte_at(bank, addr, val);
                    }
                    0x08 => s.rtc.seconds = val,
                    0x09 => s.rtc.minutes = val,
                    0x0A => s.rtc.hours = val,
                    0x0B => s.rtc.day_low = val,
                    0x0C => s.rtc.day_high = val,
                    _ => {}
                }
                self.battery_dirty |= self.header.has_battery;
            }
            MbcState::Mbc5(s) => {
                if !s.ram_enable || self.ram.is_empty() {
                    return;
                }
                let bank = s.ram_bank as usize;
                self.set_ram_byte_at(bank, addr, val);
                self.battery_dirty |= self.header.has_battery;
            }
        }
    }

    fn mbc2_nibble(&self, addr: u16) -> u8 {
        let idx = (addr as usize - 0xA000) % MBC2_RAM_NIBBLES;
        self.ram[idx] & 0x0F
    }

    fn ram_byte_at(&self, bank: usize, addr: u16) -> u8 {
        let offset = (addr as usize - 0xA000) % RAM_BANK_SIZE;
        let idx = bank * RAM_BANK_SIZE + offset;
        self.ram.get(idx).copied().unwrap_or(0xFF)
    }

    fn set_ram_byte_at(&mut self, bank: usize, addr: u16, val: u8) {
        let offset = (addr as usize - 0xA000) % RAM_BANK_SIZE;
        let idx = bank * RAM_BANK_SIZE + offset;
        if let Some(slot) = self.ram.get_mut(idx) {
            *slot = val;
        }
    }
}

fn tick_rtc_second(rtc: &mut RtcRegisters) {
    rtc.seconds = rtc.seconds.wrapping_add(1);
    if rtc.seconds < 60 {
        return;
    }
    rtc.seconds = 0;
    rtc.minutes = rtc.minutes.wrapping_add(1);
    if rtc.minutes < 60 {
        return;
    }
    rtc.minutes = 0;
    rtc.hours = rtc.hours.wrapping_add(1);
    if rtc.hours < 24 {
        return;
    }
    rtc.hours = 0;
    let mut day = ((rtc.day_high as u16 & 0x01) << 8) | rtc.day_low as u16;
    day = day.wrapping_add(1);
    if day > 0x1FF {
        day = 0;
        rtc.day_high |= 0x80; // carry flag
    }
    rtc.day_low = (day & 0xFF) as u8;
    rtc.day_high = (rtc.day_high & !0x01) | ((day >> 8) as u8 & 0x01);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid cartridge image: `rom_banks` * 16KB of
    /// zeroed data with a correct header for `cart_type`/size codes.
    fn make_rom(cart_type: u8, rom_size_code: u8, ram_size_code: u8, rom_banks: usize) -> Vec<u8> {
        let mut data = vec![0u8; rom_banks * ROM_BANK_SIZE];
        data[CART_TYPE_ADDR] = cart_type;
        data[ROM_SIZE_ADDR] = rom_size_code;
        data[RAM_SIZE_ADDR] = ram_size_code;
        let title = b"TESTGAME";
        data[TITLE_START..TITLE_START + title.len()].copy_from_slice(title);
        let checksum = (HEADER_CHECKSUM_START..HEADER_CHECKSUM_END)
            .map(|a| data[a])
            .fold(0u8, |acc, b| acc.wrapping_sub(b).wrapping_sub(1));
        data[HEADER_CHECKSUM_ADDR] = checksum;
        data
    }

    #[test]
    fn header_parses_title_type_sizes_and_valid_checksum() {
        let data = make_rom(0x00, 0x00, 0x00, 2);
        let (cart, warnings) = Cartridge::from_rom_bytes(&data);
        assert_eq!(cart.header.title, "TESTGAME");
        assert_eq!(cart.header.mbc, MbcType::None);
        assert_eq!(cart.header.rom_banks, 2);
        assert!(cart.header.header_checksum_valid);
        assert!(warnings.is_empty());
    }

    #[test]
    fn bad_checksum_and_unsupported_type_produce_warnings_but_still_load() {
        let mut data = make_rom(0xFF, 0x00, 0x00, 2); // 0xFF: no assigned MBC
        data[HEADER_CHECKSUM_ADDR] ^= 0xFF; // corrupt it
        let (cart, warnings) = Cartridge::from_rom_bytes(&data);
        assert_eq!(cart.header.mbc, MbcType::Unsupported);
        assert!(!cart.header.header_checksum_valid);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn mbc0_reads_fixed_bank0_and_bank1_with_no_switching() {
        let mut data = make_rom(0x00, 0x00, 0x00, 2);
        data[0x0000] = 0x11;
        data[0x4000] = 0x22;
        let (cart, _) = Cartridge::from_rom_bytes(&data);
        assert_eq!(cart.read_rom(0x0000), 0x11);
        assert_eq!(cart.read_rom(0x4000), 0x22);
    }

    #[test]
    fn mbc1_switches_rom_bank_and_treats_bank0_write_as_bank1() {
        let mut data = make_rom(0x01, 0x02, 0x00, 8); // 128KB, 8 banks
        data[3 * ROM_BANK_SIZE] = 0xAB; // bank 3, offset 0
        data[1 * ROM_BANK_SIZE] = 0xCD; // bank 1 (the 0->1 quirk target)
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x2000, 3);
        assert_eq!(cart.read_rom(0x4000), 0xAB);
        cart.write_rom_register(0x2000, 0); // quirk: 0 behaves as 1
        assert_eq!(cart.read_rom(0x4000), 0xCD);
    }

    #[test]
    fn mbc1_ram_requires_enable_and_banks_in_mode1() {
        let data = make_rom(0x03, 0x00, 0x03, 2); // MBC1+RAM+BATTERY, 32KB RAM
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        assert_eq!(cart.read_ram(0xA000), 0xFF); // disabled
        cart.write_rom_register(0x0000, 0x0A); // enable
        cart.write_ram(0xA000, 0x42);
        assert_eq!(cart.read_ram(0xA000), 0x42);
        assert!(cart.is_battery_dirty());

        cart.write_rom_register(0x6000, 1); // mode 1: bank2 selects RAM bank
        cart.write_rom_register(0x4000, 1); // RAM bank 1
        cart.write_ram(0xA000, 0x99);
        assert_eq!(cart.read_ram(0xA000), 0x99);
        cart.write_rom_register(0x4000, 0); // back to bank 0
        assert_eq!(cart.read_ram(0xA000), 0x42); // bank 0's byte, untouched
    }

    #[test]
    fn mbc2_rom_bank_quirk_and_nibble_ram() {
        let mut data = make_rom(0x05, 0x00, 0x00, 4);
        data[1 * ROM_BANK_SIZE] = 0x77;
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x2100, 0); // bit8 set -> ROM bank select; 0 -> quirk to 1
        assert_eq!(cart.read_rom(0x4000), 0x77);

        cart.write_rom_register(0x0000, 0x0A); // bit8 clear -> RAM enable
        cart.write_ram(0xA000, 0xFE); // only the low nibble is stored
        assert_eq!(cart.read_ram(0xA000), 0xFE); // upper nibble already 0xF
        cart.write_ram(0xA001, 0x03);
        assert_eq!(cart.read_ram(0xA001), 0xF3);
    }

    #[test]
    fn mbc3_rom_bank_quirk_and_ram_vs_rtc_register_select() {
        let mut data = make_rom(0x10, 0x01, 0x02, 4); // MBC3+RTC+RAM+BATTERY
        data[2 * ROM_BANK_SIZE] = 0x55;
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x2000, 2);
        assert_eq!(cart.read_rom(0x4000), 0x55);

        cart.write_rom_register(0x0000, 0x0A); // enable RAM/RTC access
        cart.write_rom_register(0x4000, 0x00); // select RAM bank 0
        cart.write_ram(0xA000, 0x10);
        assert_eq!(cart.read_ram(0xA000), 0x10);

        cart.write_rom_register(0x4000, 0x08); // select RTC seconds register
        cart.write_ram(0xA000, 30);
        assert_eq!(cart.read_ram(0xA000), 0); // not latched yet
        cart.write_rom_register(0x6000, 0x00);
        cart.write_rom_register(0x6000, 0x01); // latch
        assert_eq!(cart.read_ram(0xA000), 30);
    }

    /// Steps a cartridge's RTC by `cycles` T-cycles, chunked to fit
    /// `step`'s `u8` parameter.
    fn step_cart(cart: &mut Cartridge, mut cycles: u32) {
        while cycles > 0 {
            let chunk = cycles.min(255);
            cart.step(chunk as u8);
            cycles -= chunk;
        }
    }

    #[test]
    fn rtc_second_tick_rolls_over_through_minutes_hours_and_days() {
        let mut rtc = RtcRegisters { seconds: 59, minutes: 59, hours: 23, day_low: 0, day_high: 0 };
        tick_rtc_second(&mut rtc);
        assert_eq!((rtc.seconds, rtc.minutes, rtc.hours, rtc.day_low), (0, 0, 0, 1));
    }

    #[test]
    fn rtc_day_counter_overflow_sets_carry_flag() {
        // day = 511 (day_high bit0 set, day_low = 0xFF)
        let mut rtc = RtcRegisters { seconds: 59, minutes: 59, hours: 23, day_low: 0xFF, day_high: 0x01 };
        tick_rtc_second(&mut rtc);
        assert_eq!(rtc.day_low, 0);
        assert_eq!(rtc.day_high & 0x01, 0); // day counter wrapped to 0
        assert_eq!(rtc.day_high & 0x80, 0x80); // carry flag set
    }

    #[test]
    fn mbc3_step_accumulates_elapsed_cycles_into_rtc_seconds() {
        let data = make_rom(0x10, 0x00, 0x00, 2);
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        step_cart(&mut cart, CYCLES_PER_SECOND);

        cart.write_rom_register(0x0000, 0x0A); // enable RAM/RTC access
        cart.write_rom_register(0x4000, 0x08); // select RTC seconds register
        cart.write_rom_register(0x6000, 0x00);
        cart.write_rom_register(0x6000, 0x01); // latch
        assert_eq!(cart.read_ram(0xA000), 1);
    }

    #[test]
    fn mbc3_rtc_halt_flag_stops_ticking() {
        let data = make_rom(0x10, 0x00, 0x00, 2);
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x0000, 0x0A);
        cart.write_rom_register(0x4000, 0x0C); // select DH register
        cart.write_ram(0xA000, 0x40); // set halt bit
        step_cart(&mut cart, CYCLES_PER_SECOND);

        cart.write_rom_register(0x4000, 0x08); // select seconds register
        cart.write_rom_register(0x6000, 0x00);
        cart.write_rom_register(0x6000, 0x01); // latch
        assert_eq!(cart.read_ram(0xA000), 0); // still 0: RTC was halted
    }

    #[test]
    fn mbc5_uses_full_9_bit_bank_number_with_no_zero_quirk() {
        let mut data = make_rom(0x19, 0x08, 0x00, 512); // 8MB, 512 banks
        data[0 * ROM_BANK_SIZE] = 0x01; // bank 0 IS directly selectable
        data[256 * ROM_BANK_SIZE] = 0x02;
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x2000, 0x00);
        cart.write_rom_register(0x3000, 0x00);
        assert_eq!(cart.read_rom(0x4000), 0x01); // no 0->1 substitution
        cart.write_rom_register(0x2000, 0x00);
        cart.write_rom_register(0x3000, 0x01); // bit 8 set -> bank 256
        assert_eq!(cart.read_rom(0x4000), 0x02);
    }

    #[test]
    fn battery_ram_round_trips_through_save_and_load() {
        let data = make_rom(0x03, 0x00, 0x02, 2);
        let (mut cart, _) = Cartridge::from_rom_bytes(&data);
        cart.write_rom_register(0x0000, 0x0A);
        cart.write_ram(0xA000, 0x7E);
        cart.clear_battery_dirty();
        let saved = cart.battery_ram();

        let (mut fresh, _) = Cartridge::from_rom_bytes(&data);
        fresh.load_battery_ram(&saved);
        fresh.write_rom_register(0x0000, 0x0A);
        assert_eq!(fresh.read_ram(0xA000), 0x7E);
        assert!(!fresh.is_battery_dirty());
    }
}
