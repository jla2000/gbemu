//! `halt_bug` — Blargg's HALT-bug test ROM. Fetch into
//! `roms/blargg/halt_bug.gb`.

#[test]
fn halt_bug() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/halt_bug.gb");
}
