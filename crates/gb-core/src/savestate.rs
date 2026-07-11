//! Full save states: a `serde`-derived snapshot of the entire `System`
//! (CPU, PPU incl. VRAM/OAM/framebuffer, MMU incl. WRAM/HRAM, timer,
//! serial, joypad, APU incl. every channel's internal counters, and the
//! installed cartridge incl. its RAM/MBC banking registers — even its ROM
//! bytes, for a fully self-contained file), encoded with `bincode`.
//!
//! The one field that can't round-trip through serialization is the
//! `Apu`'s live connection to the frontend's audio thread (a `ringbuf`
//! producer/consumer pair — see `apu/mod.rs`): [`load_state`] swaps the
//! *current* system's audio channel back onto the freshly-deserialized
//! one before adopting it, so loading a save state never orphans an
//! already-connected audio consumer.

use crate::System;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug)]
pub enum SaveStateError {
    Encode(bincode::error::EncodeError),
    Decode(bincode::error::DecodeError),
}

impl std::fmt::Display for SaveStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveStateError::Encode(e) => write!(f, "failed to encode save state: {e}"),
            SaveStateError::Decode(e) => write!(f, "failed to decode save state: {e}"),
        }
    }
}

impl std::error::Error for SaveStateError {}

/// Serializes `system`'s full state to bytes.
pub fn save(system: &System) -> Result<Vec<u8>, SaveStateError> {
    bincode::serde::encode_to_vec(system, BINCODE_CONFIG).map_err(SaveStateError::Encode)
}

/// Deserializes `data` and adopts it into `system` in place, preserving
/// `system`'s live audio-thread connection (see the module doc).
pub fn load(system: &mut System, data: &[u8]) -> Result<(), SaveStateError> {
    let (mut loaded, _): (System, usize) =
        bincode::serde::decode_from_slice(data, BINCODE_CONFIG).map_err(SaveStateError::Decode)?;
    system.mmu.apu.swap_audio_channel(&mut loaded.mmu.apu);
    *system = loaded;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips_cpu_and_memory_state() {
        let mut sys = System::new();
        sys.load_rom(&[0x3E, 0x2A]); // LD A, 0x2A
        sys.step();
        assert_eq!(sys.cpu.regs.a, 0x2A);

        use crate::cpu::Bus;
        sys.mmu.write(0xC000, 0x99);

        let bytes = save(&sys).unwrap();

        let mut fresh = System::new();
        load(&mut fresh, &bytes).unwrap();
        assert_eq!(fresh.cpu.regs.a, 0x2A);
        assert_eq!(fresh.cpu.regs.pc, 2);
        assert_eq!(fresh.mmu.read(0xC000), 0x99);
    }

    #[test]
    fn save_then_load_round_trips_ppu_framebuffer_and_vram() {
        let mut sys = System::new();
        sys.mmu.ppu.write_lcdc(0x91); // LCD + BG on, unsigned tile addressing
        sys.mmu.ppu.write_vram(0x8000, 0xFF);
        sys.mmu.ppu.write_vram(0x8001, 0x00);
        sys.mmu.ppu.write_bgp(0xE4);
        // Run past one full scanline so the framebuffer actually has content.
        for _ in 0..2 {
            sys.step();
        }
        for _ in 0..456 {
            sys.step();
        }

        let bytes = save(&sys).unwrap();
        let mut fresh = System::new();
        load(&mut fresh, &bytes).unwrap();

        assert_eq!(fresh.mmu.ppu.framebuffer(), sys.mmu.ppu.framebuffer());
        assert_eq!(fresh.mmu.ppu.read_vram(0x8000), 0xFF);
    }

    #[test]
    fn save_then_load_round_trips_cartridge_banking_and_ram() {
        let mut data = vec![0u8; 4 * 0x4000];
        data[0x0147] = 0x03; // MBC1+RAM+BATTERY
        data[0x0148] = 0x01; // 4 banks
        data[0x0149] = 0x02; // 8KB RAM
        data[3 * 0x4000] = 0xAB;
        let checksum = (0x0134..0x014D)
            .map(|a| data[a])
            .fold(0u8, |acc, b| acc.wrapping_sub(b).wrapping_sub(1));
        data[0x014D] = checksum;

        let mut sys = System::new();
        sys.load_cartridge(&data);
        use crate::cpu::Bus;
        sys.mmu.write(0x2000, 3); // select ROM bank 3
        sys.mmu.write(0x0000, 0x0A); // enable RAM
        sys.mmu.write(0xA000, 0x77);

        let bytes = save(&sys).unwrap();
        let mut fresh = System::new();
        load(&mut fresh, &bytes).unwrap();

        assert_eq!(fresh.mmu.read(0x4000), 0xAB); // bank selection preserved
        assert_eq!(fresh.mmu.read(0xA000), 0x77); // RAM contents preserved
    }

    #[test]
    fn load_preserves_the_live_apu_audio_channel_not_the_saved_ones() {
        use ringbuf::traits::Consumer as _;

        let mut sys = System::new();
        sys.mmu.apu.write_nr52(0x80); // power on
        sys.mmu.apu.set_sample_rate(1000);
        let mut consumer = sys.mmu.apu.take_consumer().unwrap();

        let bytes = save(&sys).unwrap();
        load(&mut sys, &bytes).unwrap();

        // Step enough for at least one sample to be generated and pushed
        // (cycles_per_sample = 4_194_304 / 1000 = ~4194.3 T-cycles; NOPs
        // are 4 T-cycles each with no ROM loaded, so ~1049 steps is the
        // minimum -- comfortably clear that). If load() had swapped in a
        // fresh, disconnected producer instead of preserving the live
        // one, this consumer (obtained *before* the round trip) would
        // never see anything arrive.
        for _ in 0..2000 {
            sys.step();
        }
        assert!(consumer.try_pop().is_some());
    }
}
