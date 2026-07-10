# Test ROMs

This directory is gitignored (except this file) — the test ROMs
themselves are copyrighted and not redistributed with this repo. Fetch
them yourself into the paths below, then run the corresponding test
suite to actually verify the milestone checkboxes in `../SPEC.md` that
are currently ticked "unignored but unverified."

Every test under `tests/blargg/tests/*.rs` and (once added)
`tests/mealybug/tests/*.rs` skips gracefully (prints a notice, doesn't
fail) when its ROM file is missing, so `cargo test --workspace` stays
green without any of this.

## Blargg's test ROMs

Source: <https://github.com/retrio/gb-test-roms> (a git mirror of
blargg's original test ROM releases).

Clone or download it, then copy the pieces this project's harness looks
for:

```
roms/blargg/cpu_instrs/cpu_instrs.gb
roms/blargg/instr_timing/instr_timing.gb
roms/blargg/mem_timing/mem_timing.gb
roms/blargg/mem_timing-2/mem_timing.gb   # note: same filename as mem_timing, different dir
roms/blargg/halt_bug.gb
roms/blargg/oam_bug/oam_bug.gb
roms/blargg/dmg_sound/dmg_sound.gb
```

Then run, e.g.:

```
cargo test -p gbemu-blargg-tests cpu_instrs -- --nocapture
```

## Mealybug Tearoom tests (PPU, mid-scanline register writes)

Source: <https://github.com/mattcurrie/mealybug-tearoom-tests> — requires
building with [RGBDS](https://rgbds.gbdev.io/) (`make` in that repo);
reference screenshots ship in its `expected/` directory.

## dmg-acid2 (PPU rendering correctness)

Source: <https://github.com/mattcurrie/dmg-acid2> — a prebuilt
`dmg-acid2.gb` is attached to the repo's GitHub releases, or build it
yourself with RGBDS.

## Status as of this environment

None of the above are present here — this sandbox has no way to fetch
them, so the Blargg/Mealybug/acid2 checkboxes in `SPEC.md` that read
"unverified" are exactly that: implemented against the documented
hardware behavior, structurally un-blocked, but not run against a real
ROM. Supplying them and re-running `cargo test --workspace` is how those
get checked off for real.
