//! APU (audio processing unit): 2 pulse channels, a wave channel, a noise
//! channel, the 512Hz frame sequencer that clocks their length/envelope/
//! sweep units, and a mixer producing interleaved stereo `f32` samples
//! pushed into a lock-free ring buffer ([`ringbuf`]) for the frontend's
//! audio thread to consume — see `gb-tui`'s `audio` module for the `cpal`
//! side. `gb-core` has no dependency on `cpal` itself; the ring buffer is
//! a generic, hardware-agnostic SPSC data structure, not an audio-hardware
//! dependency, so it's fine to live here per this crate's I/O boundary.
//!
//! Not modeled (deep, narrow hardware trivia that doesn't affect real
//! game audio and isn't verifiable without `dmg_sound` test ROMs in this
//! environment): the NRx2 "zombie mode" volume glitch from writing the
//! envelope register while a channel is running, the exact
//! length-counter extra-clock edge case around powering the APU on/off
//! mid-frame-sequencer-step, and the second sweep-overflow check hardware
//! performs on every sweep-timer reload (this implements one overflow
//! check at calculation time, which is what most channels' behavior boils
//! down to in practice).

use ringbuf::traits::{Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

mod channel;
mod mixer;

use channel::{NoiseChannel, PulseChannel, WaveChannel};

/// Default output sample rate; `gb-tui` reconfigures this to the actual
/// output device's rate via [`Apu::set_sample_rate`] once it knows it.
const DEFAULT_SAMPLE_RATE: u32 = 44_100;
/// ~187ms of stereo audio — comfortably absorbs frontend scheduling
/// jitter without adding noticeable latency.
const RING_BUFFER_FRAMES: usize = 8192;

const DMG_CLOCK_HZ: f64 = 4_194_304.0;
/// Frame sequencer ticks at 512Hz: one step every 8192 T-cycles.
const FRAME_SEQUENCER_PERIOD: u32 = 8192;

const NR50_MASK: u8 = 0xFF; // all bits meaningful (VIN passthrough bits included, though unused)
const NR51_MASK: u8 = 0xFF;
const POWER_BIT: u8 = 1 << 7;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Apu {
    enabled: bool,
    nr50: u8,
    nr51: u8,
    ch1: PulseChannel,
    ch2: PulseChannel,
    ch3: WaveChannel,
    ch4: NoiseChannel,

    frame_seq_cycle_accum: u32,
    frame_seq_step: u8,

    sample_cycle_accum: f64,
    cycles_per_sample: f64,

    // Not real "system state" -- a live SPSC channel to the audio thread.
    // Skipped on save/load; `System::load_state` swaps the *live* System's
    // producer/consumer back in after deserializing, rather than adopting
    // whatever placeholder these defaults produce, so a save-state load
    // never orphans the frontend's already-connected audio consumer.
    #[serde(skip, default = "dummy_producer")]
    producer: HeapProd<f32>,
    /// Taken exactly once by the frontend's audio setup via
    /// [`Apu::take_consumer`].
    #[serde(skip)]
    consumer: Option<HeapCons<f32>>,
}

fn dummy_producer() -> HeapProd<f32> {
    HeapRb::<f32>::new(1).split().0
}

impl std::fmt::Debug for Apu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Apu")
            .field("enabled", &self.enabled)
            .field("nr50", &self.nr50)
            .field("nr51", &self.nr51)
            .field("ch1", &self.ch1)
            .field("ch2", &self.ch2)
            .field("ch3", &self.ch3)
            .field("ch4", &self.ch4)
            .finish()
    }
}

impl Clone for Apu {
    /// Clones the audio-generation state but not the ring buffer (a fresh
    /// one is created, with its consumer immediately available again) —
    /// there's no meaningful way to duplicate an in-flight SPSC channel.
    fn clone(&self) -> Self {
        let mut apu = Self::new();
        apu.enabled = self.enabled;
        apu.nr50 = self.nr50;
        apu.nr51 = self.nr51;
        apu.ch1 = self.ch1.clone();
        apu.ch2 = self.ch2.clone();
        apu.ch3 = self.ch3.clone();
        apu.ch4 = self.ch4.clone();
        apu.frame_seq_cycle_accum = self.frame_seq_cycle_accum;
        apu.frame_seq_step = self.frame_seq_step;
        apu.cycles_per_sample = self.cycles_per_sample;
        apu
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    pub fn new() -> Self {
        let rb = HeapRb::<f32>::new(RING_BUFFER_FRAMES * 2);
        let (producer, consumer) = rb.split();
        Self {
            enabled: false,
            nr50: 0,
            nr51: 0,
            ch1: PulseChannel::new(true),
            ch2: PulseChannel::new(false),
            ch3: WaveChannel::default(),
            ch4: NoiseChannel::default(),
            frame_seq_cycle_accum: 0,
            frame_seq_step: 0,
            sample_cycle_accum: 0.0,
            cycles_per_sample: DMG_CLOCK_HZ / DEFAULT_SAMPLE_RATE as f64,
            producer,
            consumer: Some(consumer),
        }
    }

    /// Takes the ring buffer's consumer half, for the frontend's audio
    /// output thread. Returns `None` if already taken.
    pub fn take_consumer(&mut self) -> Option<HeapCons<f32>> {
        self.consumer.take()
    }

    /// Swaps the live audio-channel halves (producer + whatever's left of
    /// the consumer) with `other`. Used by [`crate::system::System::load_state`]
    /// to preserve the running system's real connection to the frontend's
    /// audio thread after deserializing a save state, whose `Apu` only has
    /// a disconnected placeholder (see the `#[serde(skip)]` fields above).
    pub(crate) fn swap_audio_channel(&mut self, other: &mut Apu) {
        std::mem::swap(&mut self.producer, &mut other.producer);
        std::mem::swap(&mut self.consumer, &mut other.consumer);
    }

    /// Reconfigures the output sample rate (e.g. once `gb-tui` knows the
    /// actual audio device's rate). Takes effect immediately; any partial
    /// progress toward the next sample is kept as a dot position within
    /// the new rate, which is inaudible.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.cycles_per_sample = DMG_CLOCK_HZ / sample_rate.max(1) as f64;
    }

    pub fn read_nr10(&self) -> u8 {
        self.ch1.read_sweep()
    }
    pub fn write_nr10(&mut self, val: u8) {
        if self.enabled {
            self.ch1.write_sweep(val);
        }
    }

    pub fn read_nr11(&self) -> u8 {
        self.ch1.read_length_duty()
    }
    pub fn write_nr11(&mut self, val: u8) {
        if self.enabled {
            self.ch1.write_length_duty(val);
        }
    }

    pub fn read_nr12(&self) -> u8 {
        self.ch1.read_envelope()
    }
    pub fn write_nr12(&mut self, val: u8) {
        if self.enabled {
            self.ch1.write_envelope(val);
        }
    }

    pub fn write_nr13(&mut self, val: u8) {
        if self.enabled {
            self.ch1.write_freq_lo(val);
        }
    }

    pub fn read_nr14(&self) -> u8 {
        self.ch1.read_control()
    }
    pub fn write_nr14(&mut self, val: u8) {
        if self.enabled {
            self.ch1.write_control(val);
        }
    }

    pub fn read_nr21(&self) -> u8 {
        self.ch2.read_length_duty()
    }
    pub fn write_nr21(&mut self, val: u8) {
        if self.enabled {
            self.ch2.write_length_duty(val);
        }
    }

    pub fn read_nr22(&self) -> u8 {
        self.ch2.read_envelope()
    }
    pub fn write_nr22(&mut self, val: u8) {
        if self.enabled {
            self.ch2.write_envelope(val);
        }
    }

    pub fn write_nr23(&mut self, val: u8) {
        if self.enabled {
            self.ch2.write_freq_lo(val);
        }
    }

    pub fn read_nr24(&self) -> u8 {
        self.ch2.read_control()
    }
    pub fn write_nr24(&mut self, val: u8) {
        if self.enabled {
            self.ch2.write_control(val);
        }
    }

    pub fn read_nr30(&self) -> u8 {
        self.ch3.read_dac_power()
    }
    pub fn write_nr30(&mut self, val: u8) {
        if self.enabled {
            self.ch3.write_dac_power(val);
        }
    }

    pub fn write_nr31(&mut self, val: u8) {
        if self.enabled {
            self.ch3.write_length(val);
        }
    }

    pub fn read_nr32(&self) -> u8 {
        self.ch3.read_volume()
    }
    pub fn write_nr32(&mut self, val: u8) {
        if self.enabled {
            self.ch3.write_volume(val);
        }
    }

    pub fn write_nr33(&mut self, val: u8) {
        if self.enabled {
            self.ch3.write_freq_lo(val);
        }
    }

    pub fn read_nr34(&self) -> u8 {
        self.ch3.read_control()
    }
    pub fn write_nr34(&mut self, val: u8) {
        if self.enabled {
            self.ch3.write_control(val);
        }
    }

    pub fn write_nr41(&mut self, val: u8) {
        if self.enabled {
            self.ch4.write_length(val);
        }
    }

    pub fn read_nr42(&self) -> u8 {
        self.ch4.read_envelope()
    }
    pub fn write_nr42(&mut self, val: u8) {
        if self.enabled {
            self.ch4.write_envelope(val);
        }
    }

    pub fn read_nr43(&self) -> u8 {
        self.ch4.read_polynomial()
    }
    pub fn write_nr43(&mut self, val: u8) {
        if self.enabled {
            self.ch4.write_polynomial(val);
        }
    }

    pub fn read_nr44(&self) -> u8 {
        self.ch4.read_control()
    }
    pub fn write_nr44(&mut self, val: u8) {
        if self.enabled {
            self.ch4.write_control(val);
        }
    }

    pub fn read_nr50(&self) -> u8 {
        self.nr50
    }
    pub fn write_nr50(&mut self, val: u8) {
        if self.enabled {
            self.nr50 = val & NR50_MASK;
        }
    }

    pub fn read_nr51(&self) -> u8 {
        self.nr51
    }
    pub fn write_nr51(&mut self, val: u8) {
        if self.enabled {
            self.nr51 = val & NR51_MASK;
        }
    }

    /// Bit 7: power. Bits 4-6: unused (read 1). Bits 0-3: each channel's
    /// current enabled status (read-only).
    pub fn read_nr52(&self) -> u8 {
        let power = if self.enabled { POWER_BIT } else { 0 };
        let ch_flags = (self.ch1.enabled() as u8)
            | (self.ch2.enabled() as u8) << 1
            | (self.ch3.enabled() as u8) << 2
            | (self.ch4.enabled() as u8) << 3;
        0b0111_0000 | power | ch_flags
    }

    /// Powering off resets every channel and register (except the
    /// register itself and wave RAM, which retains its contents on real
    /// hardware); powering on resets the frame sequencer's step.
    pub fn write_nr52(&mut self, val: u8) {
        let was_enabled = self.enabled;
        self.enabled = val & POWER_BIT != 0;
        if !self.enabled && was_enabled {
            self.nr50 = 0;
            self.nr51 = 0;
            self.ch1 = PulseChannel::new(true);
            self.ch2 = PulseChannel::new(false);
            self.ch3.power_off();
            self.ch4 = NoiseChannel::default();
        } else if self.enabled && !was_enabled {
            self.frame_seq_step = 0;
        }
    }

    pub fn read_wave_ram(&self, addr: u16) -> u8 {
        self.ch3.read_wave_ram(addr)
    }
    pub fn write_wave_ram(&mut self, addr: u16, val: u8) {
        self.ch3.write_wave_ram(addr, val);
    }

    /// Advances all four channels and the frame sequencer by `t_cycles`
    /// T-cycles, and — paced by the configured sample rate — mixes and
    /// pushes stereo samples into the ring buffer. Called from
    /// `System::step` once per CPU instruction.
    pub fn step(&mut self, t_cycles: u8) {
        if !self.enabled {
            return;
        }
        self.ch1.step(t_cycles);
        self.ch2.step(t_cycles);
        self.ch3.step(t_cycles);
        self.ch4.step(t_cycles);
        self.step_frame_sequencer(t_cycles);
        self.step_sampling(t_cycles);
    }

    fn step_frame_sequencer(&mut self, t_cycles: u8) {
        self.frame_seq_cycle_accum += t_cycles as u32;
        while self.frame_seq_cycle_accum >= FRAME_SEQUENCER_PERIOD {
            self.frame_seq_cycle_accum -= FRAME_SEQUENCER_PERIOD;
            match self.frame_seq_step {
                0 | 4 => {
                    self.ch1.clock_length();
                    self.ch2.clock_length();
                    self.ch3.clock_length();
                    self.ch4.clock_length();
                }
                2 | 6 => {
                    self.ch1.clock_length();
                    self.ch2.clock_length();
                    self.ch3.clock_length();
                    self.ch4.clock_length();
                    self.ch1.clock_sweep();
                }
                7 => {
                    self.ch1.clock_envelope();
                    self.ch2.clock_envelope();
                    self.ch4.clock_envelope();
                }
                _ => {}
            }
            self.frame_seq_step = (self.frame_seq_step + 1) % 8;
        }
    }

    fn step_sampling(&mut self, t_cycles: u8) {
        self.sample_cycle_accum += t_cycles as f64;
        while self.sample_cycle_accum >= self.cycles_per_sample {
            self.sample_cycle_accum -= self.cycles_per_sample;
            let (left, right) = mixer::mix(
                self.nr50,
                self.nr51,
                self.ch1.amplitude(),
                self.ch2.amplitude(),
                self.ch3.amplitude(),
                self.ch4.amplitude(),
            );
            // Best-effort: if the consumer (audio thread) is behind and
            // the buffer is full, drop the sample rather than blocking
            // the emulation thread here -- pacing off backpressure is the
            // frontend run loop's job (see gb-tui's `audio`/`main`), this
            // is just where samples enter the buffer.
            let _ = self.producer.try_push(left);
            let _ = self.producer.try_push(right);
        }
    }

    /// Number of stereo sample-frames currently queued in the ring
    /// buffer, for the frontend's backpressure-driven pacing.
    pub fn queued_frames(&self) -> usize {
        self.producer.occupied_len() / 2
    }

    /// Ring buffer capacity in stereo sample-frames.
    pub fn buffer_capacity_frames(&self) -> usize {
        RING_BUFFER_FRAMES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::Consumer as _;

    fn powered_on_apu() -> Apu {
        let mut apu = Apu::new();
        apu.write_nr52(0x80);
        apu
    }

    #[test]
    fn power_off_clears_registers_and_disables_channels() {
        let mut apu = powered_on_apu();
        apu.write_nr50(0x77);
        apu.write_nr51(0xFF);
        apu.write_nr10(0x7F);
        apu.write_nr52(0x00); // power off
        assert_eq!(apu.read_nr50(), 0);
        assert_eq!(apu.read_nr51(), 0);
        assert_eq!(apu.read_nr52() & 0x0F, 0); // no channels enabled
    }

    #[test]
    fn writes_are_ignored_while_powered_off() {
        let mut apu = Apu::new(); // powered off by default
        apu.write_nr50(0x77);
        assert_eq!(apu.read_nr50(), 0);
    }

    #[test]
    fn nr52_reports_power_and_per_channel_enabled_bits() {
        let mut apu = powered_on_apu();
        assert_eq!(apu.read_nr52() & 0x80, 0x80);
        apu.write_nr12(0xF0); // ch1 DAC on (max volume)
        apu.write_nr14(0x80); // trigger ch1
        assert_eq!(apu.read_nr52() & 0x0F, 0b0001);
    }

    #[test]
    fn triggering_with_dac_off_does_not_enable_the_channel() {
        let mut apu = powered_on_apu();
        apu.write_nr12(0x00); // volume 0, envelope not adding: DAC off
        apu.write_nr14(0x80); // trigger
        assert_eq!(apu.read_nr52() & 0x01, 0);
    }

    #[test]
    fn wave_ram_is_readable_and_writable_regardless_of_power() {
        let mut apu = Apu::new();
        apu.write_wave_ram(0xFF30, 0xAB);
        assert_eq!(apu.read_wave_ram(0xFF30), 0xAB);
    }

    #[test]
    fn step_pushes_samples_into_the_ring_buffer_at_the_configured_rate() {
        let mut apu = powered_on_apu();
        apu.set_sample_rate(1000); // 4194.304 T-cycles/sample
        apu.write_nr50(0x77); // both channels audible on both sides
        apu.write_nr51(0xFF);
        apu.write_nr12(0xF0);
        apu.write_nr14(0x80); // trigger ch1 with a nonzero DAC volume

        let consumer = apu.take_consumer().unwrap();
        assert_eq!(consumer.occupied_len(), 0);

        // cycles_per_sample = 4_194_304 / 1000 = 4194.304; step past that.
        for _ in 0..20 {
            apu.step(255); // 5100 T-cycles total
        }
        assert!(consumer.occupied_len() >= 2); // one stereo frame = 2 f32s
    }

    #[test]
    fn silent_channels_mix_to_zero() {
        let mut apu = powered_on_apu();
        apu.set_sample_rate(1000);
        apu.write_nr50(0x77);
        apu.write_nr51(0xFF);
        // No channel triggered: everything should mix to silence.
        let mut consumer = apu.take_consumer().unwrap();
        for _ in 0..10 {
            apu.step(255);
        }
        while let Some(sample) = consumer.try_pop() {
            assert_eq!(sample, 0.0);
        }
    }
}
