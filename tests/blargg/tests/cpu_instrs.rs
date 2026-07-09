//! `cpu_instrs` — Blargg's core SM83 instruction-correctness test ROM.
//! Fetch into `roms/blargg/cpu_instrs/cpu_instrs.gb` (see workspace root
//! README / `roms/` gitignore note).

#[test]
fn cpu_instrs() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/cpu_instrs/cpu_instrs.gb");
}
