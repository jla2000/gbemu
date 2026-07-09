//! gb-core: DMG Game Boy emulation core.
//!
//! This crate contains pure emulation logic only. It must not depend on any
//! terminal, audio, or windowing library — those concerns belong to the
//! `gb-tui` frontend crate. This separation keeps the core testable headless
//! (see `tests/blargg` and `tests/mealybug` harnesses in the workspace root).

pub mod apu;
pub mod cartridge;
pub mod cpu;
pub mod joypad;
pub mod mmu;
pub mod ppu;
pub mod serial;
pub mod system;
pub mod timer;

pub use system::System;
