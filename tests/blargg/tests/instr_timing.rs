//! `instr_timing` — Blargg's instruction-timing test ROM. Fetch into
//! `roms/blargg/instr_timing/instr_timing.gb`.

#[test]
fn instr_timing() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/instr_timing/instr_timing.gb");
}
