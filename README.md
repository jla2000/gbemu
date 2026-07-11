# gbemu

A DMG (original Game Boy) emulator with a `ratatui`-based terminal
frontend — video renders via half-block Unicode characters in truecolor
RGB, with a full in-terminal debugger and real audio output.

See `SPEC.md` for the full design/architecture and milestone checklist,
and `WORKFLOW.md` for how this project is developed.

## Building and running

```
cargo build --workspace
cargo run -p gb-tui -- path/to/rom.gb
```

Requires a truecolor terminal at least 160x72 cells to render the GB
screen at full resolution (160x144 px, 2 px/cell via half-block
characters); smaller terminals show a resize prompt until you grow them.

### CLI flags

```
gbemu [ROM] [--headless] [--palette <classic|grayscale>]
```

- `ROM` — path to a `.gb` file. Optional; without one the emulator starts
  with a blank screen (useful for poking around the debugger).
- `--palette` — maps the 4 DMG shades to RGB: `classic` (green, default)
  or `grayscale`.
- `--headless` — parses args and exits without starting the TUI; used by
  the test harnesses, not meant for interactive use.

### Save files

Battery-backed cartridge RAM (and MBC3's RTC registers, where present) is
persisted to a `.sav` file next to the ROM — loaded on startup, written
on exit and periodically while dirty. Cartridges without a battery (per
their header) never touch the filesystem.

### Save states

F2/F3 (see Controls below) write/read a full save state — every byte of
CPU/PPU/MMU/APU/cartridge state, including VRAM/WRAM/OAM and the
cartridge's own RAM and MBC banking registers — to/from a single
`<rom>.state` slot next to the ROM.

## Controls

| Key(s) | Action |
|---|---|
| Arrow keys | D-pad |
| Z | A |
| X | B |
| Enter | Start |
| Right Shift | Select (needs a terminal supporting the Kitty keyboard protocol — see below) |
| Q / Esc / Ctrl-C | Quit |
| F12 | Toggle the debugger overlay |
| Tab | Cycle debugger panel (Disassembly / Registers / Memory / VRAM / Log) |
| V | Cycle the VRAM panel's sub-tab (Tiles / BG Map / OAM) |
| Space / N | Step one CPU instruction (while paused) |
| F | Step one full frame (while paused) |
| F5 | Toggle run / pause |
| B | Toggle a breakpoint at the current PC |
| W | Toggle a watchpoint at the memory viewer's cursor address |
| F2 | Save state to `<rom>.state` |
| F3 | Load state from `<rom>.state` |

Most terminals only ever report key *presses*, not holds or releases, so
D-pad/A/B/Start auto-release after a short timeout since the last press —
imperceptible while a key is actually held (OS auto-repeat keeps
refreshing it), but it's why Select specifically needs Kitty keyboard
protocol support: Shift alone isn't otherwise visible as a distinct key
at all. `gbemu` enables that protocol automatically on terminals that
advertise support for it (e.g. Kitty, WezTerm, recent iTerm2/Ghostty); on
terminals that don't, Select won't respond.

## Testing

```
cargo test --workspace
```

Runs `gb-core`'s unit tests, `gb-tui`'s unit tests (including an
end-to-end render smoke test through `ratatui`'s `TestBackend`), and the
Blargg hardware-conformance test harness. The Blargg tests skip (not
fail) when their ROM files aren't present — see `roms/README.md` for
where to get them and what re-running them with ROMs present verifies.

## Status

Actively developed milestone-by-milestone; see `SPEC.md`'s checklist for
what's implemented. As of this writing: CPU, PPU (background/window/
sprites, no cycle-accurate pixel FIFO yet), cartridge/MBC0-5 + battery
saves, joypad, OAM DMA, APU + audio output, and the debugger are all in;
the PPU-timing/audio-quirk test ROMs (`dmg-acid2`, Mealybug, `dmg_sound`
zombie-mode/sweep edge cases) are the main remaining accuracy gaps.
