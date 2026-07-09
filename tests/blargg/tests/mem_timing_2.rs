//! `mem_timing-2` — Blargg's second memory-access timing test ROM. Fetch
//! into `roms/blargg/mem_timing-2/mem_timing.gb` (the upstream repo names
//! the combined ROM `mem_timing.gb` inside the `mem_timing-2/` directory).
//!
//! Ignored until M3: this is a 64KB MBC1 multi-test build, same as
//! `cpu_instrs` — see that test's doc comment and `SPEC.md` M3.

#[test]
#[ignore = "needs M3 MBC1 ROM bank switching (64KB multi-test build)"]
fn mem_timing_2() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing-2/mem_timing.gb");
}
