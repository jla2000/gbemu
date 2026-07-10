//! `dmg_sound` — Blargg's APU test ROM. Fetch into
//! `roms/blargg/dmg_sound/dmg_sound.gb`.
//!
//! Not modeled (see `gb_core::apu`'s module doc): the NRx2 "zombie mode"
//! volume glitch, and the sweep unit's second overflow check on every
//! timer reload. Individual dmg_sound subtests targeting those specific
//! quirks are not expected to pass even once a ROM is supplied; the
//! rest (channel timing, length counters, basic envelope/sweep behavior)
//! should.

#[test]
fn dmg_sound() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/dmg_sound/dmg_sound.gb");
}
