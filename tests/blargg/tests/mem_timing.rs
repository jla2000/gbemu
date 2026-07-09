//! `mem_timing` — Blargg's memory-access timing test ROM. Fetch into
//! `roms/blargg/mem_timing/mem_timing.gb`.
//!
//! Ignored until M3: this is a 64KB MBC1 multi-test build, same as
//! `cpu_instrs` — see that test's doc comment and `SPEC.md` M3.

#[test]
#[ignore = "needs M3 MBC1 ROM bank switching (64KB multi-test build)"]
fn mem_timing() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing/mem_timing.gb");
}
