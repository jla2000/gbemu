//! Headless Blargg test-ROM harness.
//!
//! Runs a `gb_core::System` against a Blargg test ROM until either:
//! - the serial output (captured via the loopback stub in
//!   `gb_core::serial`) contains a recognizable "Passed"/"Failed" marker, or
//! - a generous cycle budget is exhausted (treated as a hang/failure).
//!
//! Test ROMs are not redistributed with this repo (see `../../roms/README`
//! or the workspace root `roms/` — gitignored). Individual test functions
//! under `tests/` skip (print + return early) rather than fail when their
//! ROM file is missing, so `cargo test --workspace` stays green without the
//! ROMs present; supplying them and re-running is how the Blargg milestone
//! checkboxes in `SPEC.md` actually get verified.

use std::path::Path;

use gb_core::System;

/// Generous upper bound on T-cycles for a single Blargg test ROM run
/// before giving up and treating it as hung. Real hardware runs these in
/// well under a second of emulated time; this is ~30 emulated seconds
/// worth of DMG cycles (4.194304 MHz), comfortably more than any of the
/// targeted suites take even including their self-test loops.
pub const CYCLE_BUDGET: u64 = 4_194_304 * 30;

/// Outcome of running a Blargg ROM to completion or budget exhaustion.
#[derive(Debug)]
pub enum Outcome {
    Passed(String),
    Failed(String),
    /// Cycle budget exhausted without a recognizable pass/fail marker —
    /// most likely an unimplemented opcode/feature causing a hang, or (for
    /// interactive-looking ROMs) one that needs input this harness can't
    /// provide.
    TimedOut(String),
}

/// Loads `rom_path`, runs it against a fresh `System`, and returns the
/// outcome. Panics if the ROM file can't be read — callers should check
/// existence first via [`skip_if_missing`] to distinguish "ROM not
/// supplied" from "test genuinely failed".
pub fn run_blargg_rom(rom_path: &Path) -> Outcome {
    let data = std::fs::read(rom_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", rom_path.display()));

    let mut sys = System::new();
    sys.load_cartridge(&data);

    let mut cycles: u64 = 0;
    while cycles < CYCLE_BUDGET {
        cycles += sys.step() as u64;

        let so_far = sys.mmu.serial.output_so_far();
        if so_far.contains("Passed") {
            return Outcome::Passed(so_far);
        }
        if so_far.contains("Failed") {
            return Outcome::Failed(so_far);
        }
    }
    Outcome::TimedOut(sys.mmu.serial.output_so_far())
}

/// Returns `Some(())` (i.e. "run the test") if `rom_path` exists, or
/// prints a skip notice and returns `None` otherwise. Test ROMs are
/// fetched by the user into `roms/` per the workspace README — not
/// redistributed here.
pub fn skip_if_missing(rom_path: &Path) -> Option<()> {
    if rom_path.exists() {
        Some(())
    } else {
        eprintln!(
            "skipping {}: ROM not found (fetch test ROMs into roms/ per README)",
            rom_path.display()
        );
        None
    }
}

/// Asserts a Blargg ROM passes, skipping (not failing) if the ROM file is
/// absent. Call from a `#[test]` fn with the ROM's path relative to the
/// workspace root.
pub fn assert_blargg_passes(rom_path: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rom_path);
    if skip_if_missing(&path).is_none() {
        return;
    }
    match run_blargg_rom(&path) {
        Outcome::Passed(out) => {
            eprintln!("{}: PASSED\n{out}", path.display());
        }
        Outcome::Failed(out) => {
            panic!("{}: FAILED\n{out}", path.display());
        }
        Outcome::TimedOut(out) => {
            panic!(
                "{}: TIMED OUT after {CYCLE_BUDGET} cycles\n{out}",
                path.display()
            );
        }
    }
}
