//! `cpu_instrs` — Blargg's core SM83 instruction-correctness test ROM.
//! Fetch into `roms/blargg/cpu_instrs/cpu_instrs.gb` (see workspace root
//! README / `roms/` gitignore note).
//!
//! This is a 64KB MBC1 multi-test build (individual subtests live in
//! separate switched-in ROM banks); the harness now loads ROMs through
//! `System::load_cartridge` (real MBC1 bank switching, M3), so this is no
//! longer blocked. `skip_if_missing` still skips (rather than fails) if
//! the ROM file itself hasn't been fetched into `roms/`.

#[test]
fn cpu_instrs() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/cpu_instrs/cpu_instrs.gb");
}
