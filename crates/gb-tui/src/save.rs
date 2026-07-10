//! Battery-backed `.sav` file persistence: derives a `.sav` path next to
//! the ROM, and loads/writes cartridge RAM (+ MBC3 RTC registers) through
//! it. Actual file I/O is a `gb-tui` concern, not `gb-core`'s — see the
//! module doc on `gb_core::cartridge` for why that boundary is drawn
//! there.

use std::path::{Path, PathBuf};

use gb_core::System;

fn sav_path_for(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
}

/// Loads a `.sav` file next to `rom_path` into the system's cartridge, if
/// a cartridge with a battery is installed and the file exists. Errors are
/// logged, not propagated — a missing or corrupt save shouldn't prevent
/// the emulator from starting.
pub fn load(system: &mut System, rom_path: &Path) {
    let Some(cart) = system.mmu.cartridge.as_mut() else { return };
    if !cart.has_battery() {
        return;
    }
    let path = sav_path_for(rom_path);
    match std::fs::read(&path) {
        Ok(data) => {
            cart.load_battery_ram(&data);
            tracing::info!("loaded battery save from {}", path.display());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("failed to read {}: {e}", path.display()),
    }
}

/// Persists the cartridge's battery RAM to a `.sav` file next to
/// `rom_path`, if a cartridge with a battery is installed. Call on exit
/// and periodically while the dirty flag is set.
pub fn persist(system: &mut System, rom_path: &Path) {
    let Some(cart) = system.mmu.cartridge.as_mut() else { return };
    if !cart.has_battery() {
        return;
    }
    let path = sav_path_for(rom_path);
    let data = cart.battery_ram();
    match std::fs::write(&path, &data) {
        Ok(()) => {
            cart.clear_battery_dirty();
            tracing::info!("wrote battery save to {}", path.display());
        }
        Err(e) => tracing::warn!("failed to write {}: {e}", path.display()),
    }
}

/// Persists only if the cartridge's battery RAM has been written to since
/// the last save (or load). Cheap to call on every loop tick.
pub fn persist_if_dirty(system: &mut System, rom_path: &Path) {
    let dirty = system.mmu.cartridge.as_ref().is_some_and(|c| c.is_battery_dirty());
    if dirty {
        persist(system, rom_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A minimal valid MBC1+RAM+BATTERY cartridge image (4 ROM banks, 1
    /// RAM bank).
    fn mbc1_battery_rom() -> Vec<u8> {
        let mut data = vec![0u8; 4 * 0x4000];
        data[0x0147] = 0x03; // MBC1+RAM+BATTERY
        data[0x0148] = 0x01; // 4 banks
        data[0x0149] = 0x02; // 8KB RAM
        let checksum = (0x0134..0x014D)
            .map(|a| data[a])
            .fold(0u8, |acc, b| acc.wrapping_sub(b).wrapping_sub(1));
        data[0x014D] = checksum;
        data
    }

    /// A unique-per-test scratch path, so parallel test runs don't collide
    /// on the same `.sav` file.
    fn scratch_rom_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("gbemu-save-test-{tag}-{n}-{}.gb", std::process::id()))
    }

    #[test]
    fn persist_then_load_round_trips_battery_ram() {
        let rom_path = scratch_rom_path("roundtrip");
        let _ = std::fs::remove_file(sav_path_for(&rom_path));

        let mut system = System::new();
        system.load_cartridge(&mbc1_battery_rom());
        {
            use gb_core::cpu::Bus;
            system.mmu.write(0x0000, 0x0A); // enable RAM
            system.mmu.write(0xA000, 0x5A);
        }
        assert!(system.mmu.cartridge.as_ref().unwrap().is_battery_dirty());

        persist(&mut system, &rom_path);
        assert!(!system.mmu.cartridge.as_ref().unwrap().is_battery_dirty());
        assert!(sav_path_for(&rom_path).exists());

        let mut fresh = System::new();
        fresh.load_cartridge(&mbc1_battery_rom());
        load(&mut fresh, &rom_path);
        {
            use gb_core::cpu::Bus;
            fresh.mmu.write(0x0000, 0x0A);
            assert_eq!(fresh.mmu.read(0xA000), 0x5A);
        }

        let _ = std::fs::remove_file(sav_path_for(&rom_path));
    }

    #[test]
    fn load_of_missing_sav_file_is_a_silent_no_op() {
        let rom_path = scratch_rom_path("missing");
        let _ = std::fs::remove_file(sav_path_for(&rom_path));

        let mut system = System::new();
        system.load_cartridge(&mbc1_battery_rom());
        load(&mut system, &rom_path); // must not panic
        assert!(!system.mmu.cartridge.as_ref().unwrap().is_battery_dirty());
    }

    #[test]
    fn persist_if_dirty_skips_the_write_when_ram_is_untouched() {
        let rom_path = scratch_rom_path("clean");
        let sav_path = sav_path_for(&rom_path);
        let _ = std::fs::remove_file(&sav_path);

        let mut system = System::new();
        system.load_cartridge(&mbc1_battery_rom());
        persist_if_dirty(&mut system, &rom_path);
        assert!(!sav_path.exists());
    }

    #[test]
    fn battery_less_cartridges_never_touch_the_filesystem() {
        let rom_path = scratch_rom_path("nobattery");
        let sav_path = sav_path_for(&rom_path);
        let _ = std::fs::remove_file(&sav_path);

        let mut data = vec![0u8; 2 * 0x4000];
        data[0x0147] = 0x00; // ROM ONLY, no battery
        let mut system = System::new();
        system.load_cartridge(&data);

        persist(&mut system, &rom_path);
        load(&mut system, &rom_path);
        assert!(!sav_path.exists());
    }
}
