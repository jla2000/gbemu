//! `oam_bug` — Blargg's OAM corruption-bug test ROM. Fetch into
//! `roms/blargg/oam_bug/oam_bug.gb`.
//!
//! Not modeled: the actual DMG "OAM bug" hardware quirk (certain 16-bit
//! `inc`/`dec` and `ldi`/`ldd` opcodes corrupt OAM when `PC` is in the
//! 0xFE00-0xFEFF range during Mode 2). This test exercises that specific
//! quirk, which this emulator doesn't yet emulate, so a real pass is not
//! expected even once a ROM is supplied — tracked here (rather than
//! `#[ignore]`d) so `cargo test` still reports its actual status once the
//! ROM is available, instead of silently skipping.

#[test]
fn oam_bug() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/oam_bug/oam_bug.gb");
}
