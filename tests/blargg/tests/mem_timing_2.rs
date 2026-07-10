//! `mem_timing-2` — Blargg's second memory-access timing test ROM. Fetch
//! into `roms/blargg/mem_timing-2/mem_timing.gb` (the upstream repo names
//! the combined ROM `mem_timing.gb` inside the `mem_timing-2/` directory).
//!
//! Same 64KB MBC1 multi-test build as `cpu_instrs` — no longer blocked
//! now that the harness loads through `System::load_cartridge` (M3).

#[test]
fn mem_timing_2() {
    gbemu_blargg_tests::assert_blargg_passes("roms/blargg/mem_timing-2/mem_timing.gb");
}
