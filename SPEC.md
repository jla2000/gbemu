# gbemu — Terminal Game Boy Emulator

A DMG (original Game Boy) emulator with a `ratatui`-based terminal frontend.
Video renders via half-block Unicode characters in truecolor RGB. Includes a
full in-terminal debugger and real audio output.

Solo/hobby project. Rust.

## Goals

- Cycle/dot-accurate enough to pass Blargg's CPU/timing test ROMs, plus
  `dmg-acid2` and Mealybug Tearoom PPU tests.
- Playable commercial DMG ROMs using MBC0/1/2/3/5 cartridges.
- Real-time audio output (4-channel APU) via `cpal`.
- Full debugger: disassembly, registers, memory viewer, VRAM/tile/OAM viewer,
  breakpoints, step/continue, in-TUI log panel.
- Full save states + battery-backed cartridge RAM (`.sav`) persistence.

## Non-goals (v1)

- No CGB (Game Boy Color) support.
- No adaptive/downscaled rendering — requires a terminal large enough for
  full 160×72-cell resolution; smaller terminals get a resize prompt, not a
  scaled-down picture.
- No gamepad/controller input, keyboard only.
- No networking / link cable emulation beyond the minimal serial stub needed
  for Blargg test ROM output capture.

## Workspace layout

Cargo workspace, two crates. `gb-core` has **zero** dependency on
ratatui/crossterm/cpal — pure emulation logic, testable headless. `gb-tui` is
the binary and owns all I/O.

```
gbemu/
├── Cargo.toml                # workspace root
├── crates/
│   ├── gb-core/
│   │   └── src/
│   │       ├── cpu/          # SM83 core: decode+execute, IME, HALT/STOP
│   │       ├── ppu/          # LCD modes, BG/window/sprite fetch, STAT
│   │       ├── apu/          # 4 channels, frame sequencer, mixer
│   │       ├── mmu/          # memory map, OAM DMA, general bus, I/O regs
│   │       ├── cartridge/    # header parsing, MBC0/1/2/3/5, save RAM
│   │       ├── timer.rs      # DIV/TIMA/TMA/TAC
│   │       ├── joypad.rs
│   │       ├── serial.rs     # link cable stub (loopback), Blargg output
│   │       ├── savestate.rs  # serde snapshot of full system state
│   │       └── system.rs     # wires components; step()/run_frame()
│   └── gb-tui/
│       └── src/
│           ├── main.rs
│           ├── app.rs        # state machine: running/paused/stepping
│           ├── audio.rs      # cpal stream, ring buffer consumer
│           ├── input.rs      # crossterm key -> joypad/debugger actions
│           ├── render/
│           │   ├── video.rs  # framebuffer -> half-block widget
│           │   └── layout.rs # screen + debug panel layout
│           └── debug/
│               ├── disasm.rs
│               ├── registers.rs
│               ├── memory.rs
│               ├── vram.rs   # tile/BG-map/OAM viewer
│               ├── breakpoints.rs
│               └── log_panel.rs
├── tests/
│   ├── blargg/                # headless serial-capture harness
│   └── mealybug/               # headless framebuffer-diff harness
└── roms/                       # gitignored; user-supplied + test ROMs
```

## Execution model

- `System::step()` executes exactly one CPU instruction, then advances
  PPU/APU/Timer by the elapsed T-cycles (4 T-cycles per M-cycle). Component
  stepping (not frame-at-a-time) is required for mid-scanline PPU register
  writes — needed to pass `dmg-acid2` and Mealybug tests.
- `System::run_frame()` calls `step()` repeatedly until PPU signals VBlank
  start, returning the completed framebuffer and any audio samples produced.
- CPU: SM83, **match-based** opcode dispatch (256 base + 256 CB-prefixed
  arms). Explicit IME/HALT-bug/STOP handling per Blargg's `halt_bug` test.
- PPU: dot-accurate mode sequence per scanline (Mode 2 OAM scan: 80 dots →
  Mode 3 Drawing: 172–289 dots, variable → Mode 0 HBlank: remainder to 456),
  repeated 144 times, then Mode 1 VBlank for 10 lines × 456 dots (4560 dots
  total). 154 scanlines/frame, 70224 dots/frame.
- APU: pulse×2, wave, noise channels + frame sequencer, mixed to f32 samples
  pushed into a lock-free ring buffer. Core paces emulation speed off
  audio-buffer backpressure (block/spin when buffer full) rather than a
  separate wall-clock timer — avoids audio/video drift.

## Rendering

- GB framebuffer: 160×144 px, 4-shade grayscale (configurable DMG
  green/gray palette, fixed RGB triples).
- Half-block technique: one terminal cell renders 2 vertical pixels via `▀`,
  foreground color = top pixel, background color = bottom pixel. Full
  resolution requires 160 cols × 72 rows for the screen area alone, more for
  debug panels.
- **No downscaling.** On startup and on every resize event, check terminal
  size. If too small, show a live "resize your terminal" prompt overlay in
  place of the emulator view; automatically resumes once the terminal is
  large enough. Poll via crossterm resize events.
- Truecolor RGB via `ratatui::style::Color::Rgb` (crossterm backend).
- Implemented as a custom `ratatui::widgets::Widget` writing buffer cells
  directly — cheaper than `Canvas`, which is built around braille/marker
  abstractions not needed for a fixed pixel grid.

## Debug UI

- Panels (tabbed or docked): **Disassembly**, **Registers/Flags**, **Memory
  viewer** (hex dump, scrollable, jump-to-address), **VRAM/Tile/OAM viewer**
  (BG tile map, tile data, sprite list), **Log panel**.
- Controls: pause/resume, step instruction, step frame, run-to-breakpoint,
  set/clear breakpoint (PC address or memory read/write watch).
- Keybinds: arrows + Z/X/Enter/RShift = D-pad/A/B/Start/Select. Space/N =
  step, F5 = run/continue, Tab = cycle panels, F12/`~` = toggle debug
  overlay.
- Logging: `tracing` + a custom `Layer` writing into an in-memory ring
  buffer, rendered by the log panel widget. No file output — stdout/stderr
  are owned by the TUI.

## Cartridge / MBC support

MBC0 (none), MBC1, MBC2, MBC3 (including RTC latch + seconds/minutes/hours/
day registers), MBC5. Header parsed at load time: title, cart type byte,
ROM/RAM size codes, optional checksum validation (warn, don't refuse).

## Save states

- **Full save states**: `serde`-derived snapshot of CPU/PPU/APU/MMU/
  cartridge state, serialized with `bincode` to a file on demand
  (load/save hotkeys).
- **Battery saves** (`.sav`): cartridge RAM + RTC only, separate from full
  save states — written on exit and on a dirty-flag interval, matching
  behavior users expect from other emulator frontends.

## Testing / correctness strategy

- **Unit tests** within `gb-core` per component (timer edge cases, MBC bank
  arithmetic, PPU mode-length math).
- **Blargg harness** (`tests/blargg`): headless `System` run; intercepts
  serial port writes (SB/SC registers) to capture ASCII output; asserts
  "Passed" appears within a cycle timeout. Covers `cpu_instrs`,
  `instr_timing`, `mem_timing`(-2), `halt_bug`, `oam_bug`, `dmg_sound`.
- **Mealybug + dmg-acid2 harness** (`tests/mealybug`): headless run to a
  fixed frame count, framebuffer dumped and pixel-diffed against reference
  PNGs checked into `tests/mealybug/reference/`. Test ROMs themselves are
  not redistributed; README documents fetching them into `roms/`
  (gitignored).
- `cargo test` runs all headless suites — no ratatui/cpal required,
  reinforcing the `gb-core`/`gb-tui` boundary.

## Key dependencies

| Crate | Version | Purpose |
|---|---|---|
| `ratatui` | 0.30 | TUI framework |
| `crossterm` | 0.29 | terminal backend (truecolor, raw mode, keys, resize events) |
| `cpal` | 0.18 | audio output |
| `serde` + `bincode` | latest | save state serialization |
| `tracing` + `tracing-subscriber` | latest | logging to in-TUI panel |
| `clap` | latest | CLI args (ROM path, `--headless`/test mode, palette flag) |
| `anyhow` / `thiserror` | latest | error handling |

## Memory map (reference)

| Range | Region |
|---|---|
| 0x0000–0x3FFF | ROM Bank 00 (fixed) |
| 0x4000–0x7FFF | ROM Bank 01–NN (switchable via MBC) |
| 0x8000–0x9FFF | VRAM (tile data + tile maps) |
| 0xA000–0xBFFF | External RAM (cartridge, switchable) |
| 0xC000–0xCFFF | WRAM Bank 0 |
| 0xD000–0xDFFF | WRAM Bank 1 |
| 0xE000–0xFDFF | Echo RAM (mirror of C000–DDFF, unused) |
| 0xFE00–0xFE9F | OAM (sprite attribute table) |
| 0xFEA0–0xFEFF | Not usable |
| 0xFF00–0xFF7F | I/O registers |
| 0xFF80–0xFFFE | HRAM |
| 0xFFFF | IE register |

---

## Milestones / Feature breakdown

### M0 — Workspace scaffold
- [x] Cargo workspace with `gb-core` (lib) and `gb-tui` (bin) crates.
- [x] CLI arg parsing (`clap`): ROM path, `--headless`, palette flag.
- [x] Empty ratatui shell: init/teardown terminal (raw mode, alt screen),
      render loop skeleton, clean exit on Ctrl-C/Q.
- [x] `tracing` wired to in-memory ring buffer (log panel stub).

### M1 — CPU core
- [x] SM83 register set, flags, match-based opcode dispatch (base + CB).
- [x] MMU stub: flat 64KB addressable bus, enough to boot test ROMs.
- [x] Interrupt handling (IE/IF, IME, dispatch priority), HALT/STOP + HALT
      bug.
- [x] Serial stub (SB/SC loopback) for test-output capture.
- [x] Timer (DIV/TIMA/TMA/TAC) + timer interrupt — pulled forward from M4:
      `instr_timing`/`mem_timing`(-2) self-time instructions via a
      genuinely-running `TIMA`, so the M1 CPU core can't be verified
      against them without it. Wired onto the MMU bus (see `mmu/mod.rs`
      doc comment), not a separate `System` field, matching how `Serial`
      already lives there.
- [x] Blargg harness passes: `instr_timing`.
- Deferred (found to need later milestones, not just M1 — see below):
  `cpu_instrs`, `mem_timing`, `mem_timing-2` need M3 MBC1 bank switching
  (64KB multi-test builds); `halt_bug` needs M2 PPU (`LY`/VBlank polling
  in Blargg's shell console). Each is `#[ignore]`d with that reason in
  `tests/blargg/tests/` until its milestone lands.

### M2 — PPU
- [x] LCDC/STAT/SCX/SCY/WX/WY/BGP/OBP0/OBP1 registers.
- [x] Dot-accurate mode sequencing (2→3→0 ×144, then Mode 1 ×10 lines).
- [x] BG + window + sprite fetch/render, tile addressing modes (8000/8800),
      OBJ-OBJ priority (X-coord + OAM index).
- [x] Half-block video widget in `gb-tui`, truecolor palette.
- [ ] Passes `dmg-acid2` and Mealybug Tearoom suite.
- [ ] Blargg harness passes: `halt_bug`. Un-ignored (no longer needs
      `LY`/VBlank polling); still unverified — no test ROM available in
      this environment. Fetch `roms/blargg/halt_bug.gb` and run
      `cargo test -p gbemu-blargg-tests halt_bug` to check this off.

### M3 — Cartridges
- [x] ROM header parsing + validation warnings.
- [x] MBC0, MBC1 (+ banking mode quirk), MBC2 (nibble RAM), MBC3 (+RTC
      latch), MBC5.
- [x] Battery-backed `.sav` load/persist (write on exit + dirty interval).
- [ ] Blargg harness passes: `cpu_instrs`, `mem_timing`, `mem_timing-2`.
      Un-ignored (harness now loads through `System::load_cartridge`, real
      MBC1 banking); still unverified — no test ROMs available in this
      environment.

### M4 — End-to-end playable
- [x] Timer (DIV/TIMA/TMA/TAC) wired to interrupts — done in M1, pulled
      forward (see above).
- [x] Joypad register + keyboard input mapping.
- [x] OAM DMA + general DMA timing.
- [ ] `oam_bug` Blargg test passes. Not expected to pass even once a ROM
      is supplied: the actual OAM-corruption hardware quirk this ROM
      exercises (certain 16-bit inc/dec/ldi/ldd opcodes corrupting OAM
      when PC is 0xFE00-0xFEFF during Mode 2) isn't modeled — narrow
      enough (real games don't rely on it) that it's being left as a
      documented gap rather than implemented speculatively.
- [ ] First playable commercial ROM, full framerate pacing. Framerate
      pacing is implemented (audio-buffer-backpressure pacing when an
      output device is available, wall-clock `run_frame()`-per-tick
      fallback otherwise — see M5) and keyboard input is wired
      end-to-end, but "first playable commercial ROM" is an experiential
      claim this environment can't verify — no ROM file and no attached
      TTY to interactively drive the real terminal UI.

### M5 — Audio
- [x] APU: pulse×2, wave, noise channels, frame sequencer.
- [x] Mixer → ring buffer → `cpal` output stream.
- [x] Emulation pacing driven by audio buffer backpressure (falls back to
      wall-clock pacing when no output device is available — this
      sandbox's usual case, verified: `cpal`'s ALSA backend finds no real
      card here).
- [ ] `dmg_sound` Blargg tests pass. Test file added; unverified — no ROM
      available in this environment. A couple of specific subtests
      (zombie-mode envelope glitch, sweep's second overflow check) aren't
      expected to pass even with a ROM — documented gaps in
      `gb_core::apu`'s module doc, narrow enough not to be worth chasing
      without a way to verify against them.

### M6 — Debugger
- [x] Disassembly panel (live, centered on PC). "Centered" is approximated
      as PC-and-forward (marked `->`) rather than truly centered — see
      the doc comment on `debug::overlay::disassembly_lines` for why
      showing bytes *before* PC is ambiguous for a variable-length
      instruction stream without a known-good alignment point.
- [x] Registers/flags panel.
- [x] Memory viewer (hex, scroll, jump-to-address via `App::mem_viewer_addr`).
- [x] VRAM/tile/BG-map/OAM viewer.
- [x] Breakpoints (PC + memory watch) and step/continue/step-frame controls.
      Watchpoints are value-change watches, not true read/write-access
      traps (would need instrumenting every `gb-core` `Bus` call) — see
      `debug::breakpoints`'s doc comment.
- [x] Log panel wired to `tracing` ring buffer.
- [x] Panel layout + keybinds (Tab cycle, F12 toggle overlay, plus
      Space/N/F/F5 step-instruction/step-frame/run-pause and B/W
      breakpoint/watchpoint toggles).

### M7 — Save states & polish
- [x] Full save-state serialization (`serde`+`bincode`), load/save hotkeys
      (F2/F3, see `gb-tui::save::quicksave`/`quickload`). Along the way,
      switched `Mmu`'s and `Ppu`'s large fixed-size byte arrays to boxed
      slices (`Box<[u8]>`) — besides avoiding large stack-resident copies
      generally, this fixed a real stack overflow in debug builds when
      deserializing a save state on a thread with a constrained stack
      (e.g. `cargo test`'s default worker threads); see the doc comment
      on `Mmu::mem`.
- [x] Resize-prompt overlay + live resize handling — already covered by
      M0's `layout::draw` (recomputes from `frame.area()` every redraw,
      so `ratatui`/`crossterm` picking up a terminal resize just works)
      plus its own render test.
- [x] Palette selection flag (M2's `--palette`), README with test-ROM
      fetch instructions (`README.md`, `roms/README.md`).
