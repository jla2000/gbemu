//! `halt_bug` — Blargg's HALT-bug test ROM. Fetch into
//! `roms/blargg/halt_bug.gb`.
//!
//! Ignored until M2: Blargg's shared shell console output routine polls
//! `LY` to pace itself on VBlank, and the PPU is still a placeholder that
//! never advances `LY` — the ROM hangs before ever reaching serial output.
//! Needs a real dot-accurate PPU mode/scanline sequencer. See `SPEC.md`
//! M2.

#[test]
#[ignore = "needs M2 PPU (LY/VBlank polled by the shell console)"]
fn halt_bug() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/halt_bug.gb");
}
