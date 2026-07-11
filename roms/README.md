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

Source: <https://github.com/mattcurrie/mealybug-tearoom-tests> — the repo
root's `mealybug-tearoom-tests.zip` has all 30 prebuilt `.gb` files (no
RGBDS build needed); reference screenshots ship in its `expected/`
directory. Extract into `roms/mealybug/`. No automated harness reads
these yet — see the M2 checkbox in `../SPEC.md`.

## dmg-acid2 (PPU rendering correctness)

Source: <https://github.com/mattcurrie/dmg-acid2/releases/tag/v1.0> — a
prebuilt `dmg-acid2.gb` is attached to the release (also `img/
reference-dmg.png`, the expected output). Fetch into
`roms/dmg-acid2/dmg-acid2.gb`, then:

```
cargo test -p gbemu-dmg-acid2-test -- --nocapture
```

## Status as of this environment

Blargg's ROMs, `dmg-acid2`, and the Mealybug ROMs are all present in this
environment as of the M2/M3 pass that verified them (see `SPEC.md`).
`cargo test --workspace` picks them up automatically; `roms/` stays
gitignored regardless, so a fresh checkout still needs this directory
populated by hand.
