//! `halt_bug` — Blargg's HALT-bug test ROM. Fetch into
//! `roms/blargg/halt_bug.gb`.
//!
//! Blargg's shared shell console output routine polls `LY` to pace itself
//! on VBlank; the M2 dot-accurate PPU mode/scanline sequencer now advances
//! `LY` for real, so this is no longer blocked. `skip_if_missing` still
//! skips (rather than fails) if the ROM file itself hasn't been fetched
//! into `roms/`.

#[test]
fn halt_bug() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/halt_bug.gb");
}
