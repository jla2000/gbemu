//! `cpu_instrs` — Blargg's core SM83 instruction-correctness test ROM.
//! Fetch into `roms/blargg/cpu_instrs/cpu_instrs.gb` (see workspace root
//! README / `roms/` gitignore note).
//!
//! Ignored until M3: this is a 64KB MBC1 multi-test build (individual
//! subtests live in separate switched-in ROM banks) — the M1 flat 64KB MMU
//! has no bank switching, so bytes meant for banks 2/3 land in
//! VRAM/WRAM/I-O space instead. Needs real MBC1 ROM bank switching. See
//! `SPEC.md` M3.

#[test]
#[ignore = "needs M3 MBC1 ROM bank switching (64KB multi-test build)"]
fn cpu_instrs() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/cpu_instrs/cpu_instrs.gb");
}
