//! `dmg-acid2` — Matt Currie's DMG PPU rendering-correctness test ROM
//! (background/window/sprite priority, tile addressing modes, etc., all
//! exercised at once by a single static image). Fetch into
//! `roms/dmg-acid2/dmg-acid2.gb` (see `../../roms/README.md`).
//!
//! Unlike the Blargg suites, this ROM doesn't report a pass/fail verdict
//! itself — it just renders a fixed image and holds it. Verification is
//! done by rendering headlessly for long enough for the image to settle,
//! then checking the resulting framebuffer's CRC32 against the one
//! computed from the project's official reference screenshot
//! (`https://github.com/mattcurrie/dmg-acid2/blob/master/img/reference-dmg.png`,
//! decoded to the same shade-index-per-pixel form `gb_core::ppu::Ppu`
//! produces). The reference image itself isn't redistributed here (only
//! its checksum), same as Blargg's ROMs aren't.

use std::path::Path;

use gb_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use gb_core::System;

/// CRC32 (IEEE 802.3 / zlib) of the reference-dmg.png image, decoded to
/// one DMG shade index (0 = lightest .. 3 = darkest) per pixel,
/// row-major, `SCREEN_WIDTH` x `SCREEN_HEIGHT`.
const EXPECTED_CRC32: u32 = 0x9f51dd25;

/// Generous frame budget for the image to finish drawing and settle into
/// its final static state; the actual ROM does this in well under a
/// second of emulated time.
const SETTLE_FRAMES: u32 = 60;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[test]
fn dmg_acid2() {
    let rom_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../roms/dmg-acid2/dmg-acid2.gb");
    if !rom_path.exists() {
        eprintln!(
            "skipping dmg_acid2: ROM not found at {} (see roms/README.md)",
            rom_path.display()
        );
        return;
    }

    let data = std::fs::read(&rom_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", rom_path.display()));

    let mut sys = System::new();
    sys.load_cartridge(&data);
    for _ in 0..SETTLE_FRAMES {
        sys.run_frame();
    }

    let fb: &[u8; SCREEN_WIDTH * SCREEN_HEIGHT] = sys.mmu.ppu.framebuffer();
    let actual_crc32 = crc32(fb);
    assert_eq!(
        actual_crc32, EXPECTED_CRC32,
        "rendered framebuffer doesn't match the dmg-acid2 reference image \
         (crc32 {actual_crc32:#010x} != expected {EXPECTED_CRC32:#010x})"
    );
}

