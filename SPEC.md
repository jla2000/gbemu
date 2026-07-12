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

- **Always-on status sidebar** (M8): CPU registers/flags, key PPU/Timer/
  Cartridge/APU state render unconditionally next to the video area — no
  toggle needed to see what the machine is doing right now.
- Heavier panels (togglable via F12, tabbed via Tab): **Disassembly**,
  **Memory viewer** (hex dump, scrollable, jump-to-address), **VRAM/Tile/
  OAM viewer** (BG tile map, tile data, sprite list), **Log panel**. (The
  standalone **Registers/Flags** panel from M6 is superseded by the M8
  sidebar, which shows the same data unconditionally — see M8.)
- Controls: pause/resume, step instruction, step frame, run-to-breakpoint,
  set/clear breakpoint (PC address or memory read/write watch).
- Keybinds: arrows + Z/X/Enter/RShift = D-pad/A/B/Start/Select. Space/N =
  step, F5 = run/continue, Tab = cycle panels, F12/`~` = toggle the
  heavier panel set (the status sidebar itself is never hidden).
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
- [x] Passes `dmg-acid2` (`roms/dmg-acid2/dmg-acid2.gb`, fetched from
      <https://github.com/mattcurrie/dmg-acid2/releases/tag/v1.0>).
      Verified with a new automated test, `tests/dmg_acid2` — renders
      headlessly for 60 frames, then checks the resulting framebuffer's
      CRC32 against the one computed from the project's official
      `reference-dmg.png`; byte-for-byte exact match, 0/23040 pixels
      differ. Finding this required its own fix: `System::run_frame` (and
      the TUI's `run_frame_checking_breakpoints`) used to stop dead the
      instant the LCD was disabled mid-frame — completely normal ROM
      behavior (disable LCD, set up VRAM, re-enable it) that `dmg-acid2`
      itself does — because every subsequent call would see the LCD still
      off and refuse to step the CPU even once, forever. Now paced by a
      fixed per-frame dot budget (`ppu::DOTS_PER_FRAME`) instead of
      watching for a VBlank edge, so the CPU always keeps running
      regardless of LCD state.
- [ ] Passes the Mealybug Tearoom suite. ROMs fetched (30 prebuilt `.gb`
      files in `roms/mealybug/`, from
      <https://github.com/mattcurrie/mealybug-tearoom-tests>'s bundled
      `mealybug-tearoom-tests.zip`), but no automated harness/reference
      images wired up yet (each of the 30 tests needs its own expected
      screenshot from that repo's `expected/` directory) — left as a
      follow-up given the size of that harness relative to this pass.
- [ ] Blargg harness passes: `halt_bug`. Verified failing (ROM now present
      in `roms/blargg/halt_bug.gb`) — not a hang/timeout, a real reported
      failure; root cause not yet diagnosed (no bundled source for this
      ROM to cross-reference).

### M3 — Cartridges
- [x] ROM header parsing + validation warnings.
- [x] MBC0, MBC1 (+ banking mode quirk), MBC2 (nibble RAM), MBC3 (+RTC
      latch), MBC5.
- [x] Battery-backed `.sav` load/persist (write on exit + dirty interval).
- [x] Blargg harness passes: `cpu_instrs`. Was blocked on two real bugs,
      both fixed: (1) `System::load_cartridge` never initialized CPU/PPU
      registers to the DMG post-boot state, so the CPU started executing
      from `PC=0x0000` with the LCD off instead of the cartridge's real
      `0x0100` entry point — see `System::power_on_post_boot`; (2) opcode
      `0xF8` (`LD HL,SP+e8`) was implemented as a duplicate of `0xE8`
      (`ADD SP,e8` — mutating `SP` itself instead of writing the sum to
      `HL`), which silently corrupted the stack whenever Blargg's shared
      console routines used it, cascading into unrelated subtest failures.
      Also needed the harness's `CYCLE_BUDGET` raised (the combined
      multi-bank ROM legitimately takes ~50 emulated seconds, more than
      the old 30s budget) and support for the `shell.s`-family ROMs'
      cartridge-RAM result-reporting protocol (see `ram_report.rs`) for
      the suites below that use it instead of the serial port.
- [ ] Blargg harness passes: `mem_timing`, `mem_timing-2`. Verified
      failing, root cause diagnosed: these test *which specific M-cycle
      within an instruction* a memory access happens on (e.g. `LDH
      A,(a8)`'s read should land on M-cycle 3 of 3), but `Cpu::step`
      executes an instruction's bus reads/writes synchronously and only
      bulk-advances the timer/PPU/etc. by the total elapsed cycles
      afterward — there's no per-M-cycle interleaving for these tests to
      observe. Fixing this for real needs a larger step-execution
      refactor, not a targeted opcode fix.

### M4 — End-to-end playable
- [x] Timer (DIV/TIMA/TMA/TAC) wired to interrupts — done in M1, pulled
      forward (see above).
- [x] Joypad register + keyboard input mapping.
- [x] OAM DMA + general DMA timing.
- [ ] `oam_bug` Blargg test passes. Verified failing as expected (ROM now
      present in `roms/blargg/oam_bug/oam_bug.gb`): the actual
      OAM-corruption hardware quirk this ROM exercises (certain 16-bit
      inc/dec/ldi/ldd opcodes corrupting OAM when PC is 0xFE00-0xFEFF
      during Mode 2) isn't modeled — narrow enough (real games don't rely
      on it) that it's being left as a documented gap rather than
      implemented speculatively.
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
- [ ] `dmg_sound` Blargg tests pass. ROM now present
      (`roms/blargg/dmg_sound/dmg_sound.gb`), but the run itself is broken:
      it doesn't hit the cycle budget's exit path within any reasonable
      wall-clock time (10+ minutes of real CPU time observed with no
      result), unlike every other suite (all well under a few seconds for
      a full 90-emulated-second budget). Not yet root-caused — likely
      either a genuine infinite loop specific to this ROM's APU self-tests
      or a performance problem in `Apu::step`/the mixer, not investigated
      further here. A couple of specific subtests (zombie-mode envelope
      glitch, sweep's second overflow check) aren't expected to pass even
      once this is fixed — documented gaps in `gb_core::apu`'s module doc.

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

### M8 — Always-on status sidebar & richer debug UI
Today `App::debug_overlay` (F12) gates *all* debug info, registers
included — you can't see what the CPU is doing without dedicating half
the terminal to a tabbed panel. This milestone splits that: a compact
status sidebar with CPU/PPU/Timer/Cartridge/APU state renders
unconditionally, and the heavier panels (disassembly, memory hex dump,
VRAM viewer, log) stay behind the existing F12 toggle since they need
more space and aren't relevant every frame. Also an opportunity for
some visual polish beyond plain hex dumps — decoded flags, color-coding,
change highlighting — since "interesting" was explicitly asked for, not
just "more."

- [x] New always-on `StatusSidebarWidget` (new module, e.g.
      `gb-tui/src/debug/status.rs`), rendered unconditionally beside the
      video area regardless of `App::debug_overlay`. Sections, all backed
      by data already exposed publicly (no `gb-core` changes needed for
      this item):
      - **CPU**: AF/BC/DE/HL/SP/PC, Z/N/H/C flags, IME, HALT/STOP —
        today's `debug::registers::lines` content, relocated here.
      - **PPU**: LCDC, STAT (raw + decoded mode 0-3), LY/LYC, SCX/SCY/
        WX/WY, BGP/OBP0/OBP1 — all via `Ppu`'s existing `read_*` methods.
      - **Timer**: DIV/TIMA/TMA/TAC via `Timer`'s existing `read_*`
        methods.
      - **Joypad**: currently-held buttons, from `App::button_last_pressed`
        (already tracked, just not displayed).
      - **Cartridge**: title, MBC type, ROM/RAM bank *counts* — all on
        `Cartridge::header` (`pub struct Header`), already public.
      - **APU**: per-channel on/off, decoded from the low 4 bits of
        `Apu::read_nr52()` (already public); NR50/NR51 summary
        (master volume / panning) via `read_nr50`/`read_nr51`.
      - **Run state**: RUNNING/PAUSED + breakpoint/watchpoint counts,
        moved up from the status line (today's
        `debug::overlay::status_summary`) into the sidebar header so
        it's grouped with the rest of the always-on info.
- [x] Layout rework in `render/layout.rs`: video (160x72) + sidebar
      (64 cols — wider than the originally-suggested ~32-34, since the
      decoded LCDC line alone runs ~60 columns) share the top row
      unconditionally. The existing F12-toggled `DebugOverlayWidget`
      (disassembly/memory/vram/log, still Tab-cycled) moves from *beside*
      the video to a full-width panel *below* it, shown only when
      `debug_overlay` is on — avoids requiring an even-wider terminal
      just to toggle it. `layout::MIN_COLS` is now `SCREEN_COLS +
      SIDEBAR_COLS` (sidebar space is unconditional now, not gated on
      `debug_overlay`); `MIN_ROWS` unchanged when the overlay is off,
      grows by `MIN_OVERLAY_ROWS` when it's on (same idea as the old
      horizontal split, just reoriented to vertical).
- [x] Retired the standalone `DebugPanel::Registers` tab (superseded by
      the sidebar) from `App`'s panel-cycling; the old `debug::registers`
      module's content moved into the sidebar (`debug::status::cpu_lines`)
      rather than staying duplicated in both places, and the module
      itself was deleted.
- [x] Decode `LCDC`/`STAT` into readable flags alongside the raw hex
      (e.g. `LCDC F3  LCD:ON BG:ON WIN:OFF OBJ:ON(8x8) MAP:9800
      TILE:8000`), rather than just the raw byte value — same spirit as
      today's `AF: 1234 (A:12 F:34)` register-pair decoding.
- [x] Color-code sidebar section titles/borders by category (CPU/PPU/
      Timer/Cartridge/APU each a distinct, consistent color) so it reads
      at a glance instead of as a wall of undifferentiated hex.
- [x] Changed-value highlighting: `App` keeps the previous frame's CPU
      register snapshot; the sidebar renders any register that changed
      since the last redraw in a highlight color for that frame, making
      execution activity visible without single-stepping.
- [x] Update tests that assumed the old layout/panel set: `render::
      layout`'s resize-prompt tests (new `MIN_COLS`/`MIN_ROWS_WITH_OVERLAY`),
      `main.rs`'s full-render-pipeline smoke test (drop
      `DebugPanel::Registers` from the panels it iterates — the sidebar
      render pass happens on every draw already), `debug::overlay`'s
      `status_summary` test (removed; its content moved into
      `debug::status::run_state_line`, covered there). `SPEC.md`'s
      "Debug UI" section already described this end state.
- [ ] Stretch, deferred unless time allows — both need small new
      `gb-core` accessors, not exposed today: a live MBC bank indicator
      (current ROM/RAM bank in use; each MBC's bank-select state is
      private) and a per-APU-channel output-level meter
      (`ratatui::widgets::Sparkline`/`Gauge`; channels don't expose live
      output amplitude, only on/off).
