//! `mem_timing-2` — Blargg's second memory-access timing test ROM. Fetch
//! into `roms/blargg/mem_timing-2/mem_timing-2.gb`.

#[test]
fn mem_timing_2() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing-2/mem_timing-2.gb");
}
