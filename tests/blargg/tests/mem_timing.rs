//! `mem_timing` — Blargg's memory-access timing test ROM. Fetch into
//! `roms/blargg/mem_timing/mem_timing.gb`.
//!
//! Same 64KB MBC1 multi-test build as `cpu_instrs` — no longer blocked
//! now that the harness loads through `System::load_cartridge` (M3).

#[test]
fn mem_timing() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing/mem_timing.gb");
}
