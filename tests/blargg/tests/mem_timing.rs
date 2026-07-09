//! `mem_timing` — Blargg's memory-access timing test ROM. Fetch into
//! `roms/blargg/mem_timing/mem_timing.gb`.

#[test]
fn mem_timing() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing/mem_timing.gb");
}
