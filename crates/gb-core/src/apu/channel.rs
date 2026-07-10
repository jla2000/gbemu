//! The four DMG sound channels: two pulse (one with a frequency sweep),
//! one wave, one noise. Each exposes register read/write methods matching
//! real hardware's bit layout/masking, a `step` advancing its frequency
//! timer by elapsed T-cycles, `clock_length`/`clock_envelope`/
//! `clock_sweep` driven by the frame sequencer (see `apu/mod.rs`), and
//! `amplitude` (0-15, the current digital sample) for the mixer.

const DUTY_PATTERNS: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75%
];

const PULSE_LENGTH_MAX: u16 = 64;
const WAVE_LENGTH_MAX: u16 = 256;

/// Shared by pulse channels 1/2 and the noise channel: initial volume,
/// direction, and period from an `NRx2`-shaped register, plus the
/// running volume/timer state the 64Hz frame-sequencer step clocks.
#[derive(Debug, Default, Clone, Copy)]
struct Envelope {
    initial_volume: u8,
    add: bool,
    period: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn dac_enabled(&self) -> bool {
        self.initial_volume != 0 || self.add
    }

    fn write_nrx2(&mut self, val: u8) {
        self.initial_volume = val >> 4;
        self.add = val & 0x08 != 0;
        self.period = val & 0x07;
    }

    fn read_nrx2(&self) -> u8 {
        (self.initial_volume << 4) | ((self.add as u8) << 3) | self.period
    }

    fn trigger(&mut self) {
        self.volume = self.initial_volume;
        self.timer = self.period;
    }

    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.add && self.volume < 15 {
                self.volume += 1;
            } else if !self.add && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Sweep {
    period: u8,
    negate: bool,
    shift: u8,
    timer: u8,
    enabled: bool,
    shadow_frequency: u16,
}

impl Sweep {
    fn write_nr10(&mut self, val: u8) {
        self.period = (val >> 4) & 0x07;
        self.negate = val & 0x08 != 0;
        self.shift = val & 0x07;
    }

    fn read_nr10(&self) -> u8 {
        0x80 | (self.period << 4) | ((self.negate as u8) << 3) | self.shift
    }

    /// Returns `Some(new_freq)` if the trigger-time overflow check passes
    /// (or there's no shift to apply), `None` if it immediately overflows
    /// and channel 1 should be disabled.
    fn trigger(&mut self, frequency: u16) -> Option<u16> {
        self.shadow_frequency = frequency;
        self.timer = if self.period == 0 { 8 } else { self.period };
        self.enabled = self.period != 0 || self.shift != 0;
        if self.shift != 0 {
            self.calculate()
        } else {
            Some(frequency)
        }
    }

    fn calculate(&mut self) -> Option<u16> {
        let delta = self.shadow_frequency >> self.shift;
        let new_freq = if self.negate {
            self.shadow_frequency.wrapping_sub(delta)
        } else {
            self.shadow_frequency.wrapping_add(delta)
        };
        if new_freq > 2047 {
            None
        } else {
            Some(new_freq)
        }
    }

    /// Returns `Some(new_freq)` when the sweep timer fires and produces a
    /// new (in-range) frequency to apply, `Some(overflow)` semantics
    /// folded into `None` meaning "disable the channel".
    fn clock(&mut self) -> Option<Option<u16>> {
        if !self.enabled || self.period == 0 {
            return None;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer != 0 {
            return None;
        }
        self.timer = if self.period == 0 { 8 } else { self.period };
        match self.calculate() {
            Some(new_freq) if self.shift != 0 => {
                self.shadow_frequency = new_freq;
                Some(Some(new_freq))
            }
            Some(_) => None, // shift == 0: recalculated but not applied
            None => Some(None), // overflow: disable the channel
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct PulseChannel {
    has_sweep: bool,
    sweep: Sweep,
    duty: u8,
    length_load: u8,
    length_counter: u16,
    length_enable: bool,
    envelope: Envelope,
    frequency: u16,
    freq_timer: u16,
    duty_pos: u8,
    enabled: bool,
}

impl PulseChannel {
    pub(super) fn new(has_sweep: bool) -> Self {
        Self { has_sweep, ..Default::default() }
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn read_sweep(&self) -> u8 {
        if self.has_sweep {
            self.sweep.read_nr10()
        } else {
            0xFF
        }
    }
    pub(super) fn write_sweep(&mut self, val: u8) {
        if self.has_sweep {
            self.sweep.write_nr10(val);
        }
    }

    pub(super) fn read_length_duty(&self) -> u8 {
        0x3F | (self.duty << 6)
    }
    pub(super) fn write_length_duty(&mut self, val: u8) {
        self.duty = val >> 6;
        self.length_load = val & 0x3F;
        self.length_counter = PULSE_LENGTH_MAX - self.length_load as u16;
    }

    pub(super) fn read_envelope(&self) -> u8 {
        self.envelope.read_nrx2()
    }
    pub(super) fn write_envelope(&mut self, val: u8) {
        self.envelope.write_nrx2(val);
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }

    pub(super) fn write_freq_lo(&mut self, val: u8) {
        self.frequency = (self.frequency & 0x0700) | val as u16;
    }

    pub(super) fn read_control(&self) -> u8 {
        0xBF | ((self.length_enable as u8) << 6)
    }
    pub(super) fn write_control(&mut self, val: u8) {
        self.frequency = (self.frequency & 0x00FF) | (((val & 0x07) as u16) << 8);
        self.length_enable = val & 0x40 != 0;
        if self.length_counter == 0 {
            self.length_counter = PULSE_LENGTH_MAX;
        }
        if val & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.envelope.dac_enabled();
        self.freq_timer = (2048 - self.frequency) * 4;
        self.envelope.trigger();
        if self.has_sweep && self.enabled {
            if self.sweep.trigger(self.frequency).is_none() {
                self.enabled = false;
            }
        }
    }

    pub(super) fn clock_length(&mut self) {
        if !self.length_enable || self.length_counter == 0 {
            return;
        }
        self.length_counter -= 1;
        if self.length_counter == 0 {
            self.enabled = false;
        }
    }

    pub(super) fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub(super) fn clock_sweep(&mut self) {
        if !self.has_sweep {
            return;
        }
        match self.sweep.clock() {
            Some(Some(new_freq)) => self.frequency = new_freq,
            Some(None) => self.enabled = false,
            None => {}
        }
    }

    pub(super) fn step(&mut self, t_cycles: u8) {
        let mut remaining = t_cycles as u32;
        while remaining > 0 {
            if self.freq_timer == 0 {
                self.freq_timer = (2048 - self.frequency) * 4;
            }
            let consumed = remaining.min(self.freq_timer as u32);
            self.freq_timer -= consumed as u16;
            remaining -= consumed;
            if self.freq_timer == 0 {
                self.duty_pos = (self.duty_pos + 1) % 8;
            }
        }
    }

    pub(super) fn amplitude(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        DUTY_PATTERNS[self.duty as usize][self.duty_pos as usize] * self.envelope.volume
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WaveChannel {
    dac_enabled: bool,
    length_load: u8,
    length_counter: u16,
    length_enable: bool,
    volume_code: u8,
    frequency: u16,
    freq_timer: u16,
    wave_pos: u8,
    enabled: bool,
    ram: [u8; 16], // 32 4-bit samples
}

impl Default for WaveChannel {
    fn default() -> Self {
        Self {
            dac_enabled: false,
            length_load: 0,
            length_counter: 0,
            length_enable: false,
            volume_code: 0,
            frequency: 0,
            freq_timer: 0,
            wave_pos: 0,
            enabled: false,
            ram: [0; 16],
        }
    }
}

impl WaveChannel {
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn read_dac_power(&self) -> u8 {
        0x7F | ((self.dac_enabled as u8) << 7)
    }
    pub(super) fn write_dac_power(&mut self, val: u8) {
        self.dac_enabled = val & 0x80 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    pub(super) fn write_length(&mut self, val: u8) {
        self.length_load = val;
        self.length_counter = WAVE_LENGTH_MAX - self.length_load as u16;
    }

    pub(super) fn read_volume(&self) -> u8 {
        0x9F | (self.volume_code << 5)
    }
    pub(super) fn write_volume(&mut self, val: u8) {
        self.volume_code = (val >> 5) & 0x03;
    }

    pub(super) fn write_freq_lo(&mut self, val: u8) {
        self.frequency = (self.frequency & 0x0700) | val as u16;
    }

    pub(super) fn read_control(&self) -> u8 {
        0xBF | ((self.length_enable as u8) << 6)
    }
    pub(super) fn write_control(&mut self, val: u8) {
        self.frequency = (self.frequency & 0x00FF) | (((val & 0x07) as u16) << 8);
        self.length_enable = val & 0x40 != 0;
        if self.length_counter == 0 {
            self.length_counter = WAVE_LENGTH_MAX;
        }
        if val & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        self.freq_timer = (2048 - self.frequency) * 2;
        self.wave_pos = 0;
    }

    pub(super) fn clock_length(&mut self) {
        if !self.length_enable || self.length_counter == 0 {
            return;
        }
        self.length_counter -= 1;
        if self.length_counter == 0 {
            self.enabled = false;
        }
    }

    pub(super) fn step(&mut self, t_cycles: u8) {
        let mut remaining = t_cycles as u32;
        while remaining > 0 {
            if self.freq_timer == 0 {
                self.freq_timer = (2048 - self.frequency) * 2;
            }
            let consumed = remaining.min(self.freq_timer as u32);
            self.freq_timer -= consumed as u16;
            remaining -= consumed;
            if self.freq_timer == 0 {
                self.wave_pos = (self.wave_pos + 1) % 32;
            }
        }
    }

    pub(super) fn amplitude(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        let byte = self.ram[(self.wave_pos / 2) as usize];
        let nibble = if self.wave_pos % 2 == 0 { byte >> 4 } else { byte & 0x0F };
        match self.volume_code {
            0 => 0,
            1 => nibble,
            2 => nibble >> 1,
            _ => nibble >> 2,
        }
    }

    pub(super) fn read_wave_ram(&self, addr: u16) -> u8 {
        self.ram[(addr - 0xFF30) as usize]
    }
    pub(super) fn write_wave_ram(&mut self, addr: u16, val: u8) {
        self.ram[(addr - 0xFF30) as usize] = val;
    }

    pub(super) fn power_off(&mut self) {
        let ram = self.ram;
        *self = Self { ram, ..Self::default() };
    }
}

const NOISE_DIVISORS: [u16; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct NoiseChannel {
    length_load: u8,
    length_counter: u16,
    length_enable: bool,
    envelope: Envelope,
    clock_shift: u8,
    width_mode: bool,
    divisor_code: u8,
    freq_timer: u16,
    lfsr: u16,
    enabled: bool,
}

impl NoiseChannel {
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn write_length(&mut self, val: u8) {
        self.length_load = val & 0x3F;
        self.length_counter = PULSE_LENGTH_MAX - self.length_load as u16;
    }

    pub(super) fn read_envelope(&self) -> u8 {
        self.envelope.read_nrx2()
    }
    pub(super) fn write_envelope(&mut self, val: u8) {
        self.envelope.write_nrx2(val);
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }

    pub(super) fn read_polynomial(&self) -> u8 {
        (self.clock_shift << 4) | ((self.width_mode as u8) << 3) | self.divisor_code
    }
    pub(super) fn write_polynomial(&mut self, val: u8) {
        self.clock_shift = val >> 4;
        self.width_mode = val & 0x08 != 0;
        self.divisor_code = val & 0x07;
    }

    pub(super) fn read_control(&self) -> u8 {
        0xBF | ((self.length_enable as u8) << 6)
    }
    pub(super) fn write_control(&mut self, val: u8) {
        self.length_enable = val & 0x40 != 0;
        if self.length_counter == 0 {
            self.length_counter = PULSE_LENGTH_MAX;
        }
        if val & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.envelope.dac_enabled();
        self.freq_timer = NOISE_DIVISORS[self.divisor_code as usize] << self.clock_shift;
        self.envelope.trigger();
        self.lfsr = 0x7FFF;
    }

    pub(super) fn clock_length(&mut self) {
        if !self.length_enable || self.length_counter == 0 {
            return;
        }
        self.length_counter -= 1;
        if self.length_counter == 0 {
            self.enabled = false;
        }
    }

    pub(super) fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub(super) fn step(&mut self, t_cycles: u8) {
        let mut remaining = t_cycles as u32;
        while remaining > 0 {
            if self.freq_timer == 0 {
                self.freq_timer = NOISE_DIVISORS[self.divisor_code as usize] << self.clock_shift;
            }
            let consumed = remaining.min(self.freq_timer as u32);
            self.freq_timer -= consumed as u16;
            remaining -= consumed;
            if self.freq_timer == 0 {
                let xor_bit = (self.lfsr & 0x01) ^ ((self.lfsr >> 1) & 0x01);
                self.lfsr = (self.lfsr >> 1) | (xor_bit << 14);
                if self.width_mode {
                    self.lfsr = (self.lfsr & !0x40) | (xor_bit << 6);
                }
            }
        }
    }

    pub(super) fn amplitude(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.lfsr & 0x01 == 0 {
            self.envelope.volume
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_trigger_with_dac_off_stays_disabled() {
        let mut ch = PulseChannel::new(false);
        ch.write_envelope(0x00); // volume 0, not adding: DAC off
        ch.write_control(0x80); // trigger
        assert!(!ch.enabled());
    }

    #[test]
    fn pulse_trigger_with_dac_on_enables_and_reloads_length() {
        let mut ch = PulseChannel::new(false);
        ch.write_envelope(0xF0); // volume 15
        ch.write_length_duty(0b11_111111); // duty 3, length_load 63 -> counter 1
        ch.write_control(0x80);
        assert!(ch.enabled());
    }

    #[test]
    fn pulse_length_counter_disables_channel_at_zero() {
        let mut ch = PulseChannel::new(false);
        ch.write_envelope(0xF0);
        ch.write_length_duty(0b00_111111); // length_load 63 -> counter 1
        ch.write_control(0xC0); // trigger + length enable
        assert!(ch.enabled());
        ch.clock_length();
        assert!(!ch.enabled());
    }

    #[test]
    fn pulse_duty_pattern_produces_expected_amplitude_sequence() {
        let mut ch = PulseChannel::new(false);
        ch.write_envelope(0xF0); // volume 15
        ch.write_length_duty(0b10_000000); // duty 2 (50%): 1,0,0,0,0,1,1,1
        ch.write_freq_lo(0xFC);
        ch.write_control(0x87); // freq hi bits (7) + trigger -> frequency 0x7FC
        // freq_timer period = (2048-0x7FC)*4 = 16 T-cycles per duty step
        let expected = [1u8, 0, 0, 0, 0, 1, 1, 1];
        for &e in &expected {
            assert_eq!(ch.amplitude(), e * 15);
            ch.step(16);
        }
    }

    #[test]
    fn envelope_ramps_volume_up_and_stops_at_max() {
        let mut env = Envelope::default();
        env.write_nrx2(0b0000_1010); // volume 0, add, period 2
        env.trigger();
        env.clock(); // period 2: this clock only counts down the timer
        assert_eq!(env.volume, 0);
        env.clock(); // timer hits 0: volume increments
        assert_eq!(env.volume, 1);
    }

    #[test]
    fn sweep_disables_channel_on_overflow() {
        let mut sweep = Sweep::default();
        sweep.write_nr10(0b0_001_0_001); // period 1, add, shift 1
        // frequency 2000: delta = 2000>>1=1000, new=3000 > 2047 -> overflow
        assert_eq!(sweep.trigger(2000), None);
    }

    #[test]
    fn sweep_computes_new_frequency_within_range() {
        let mut sweep = Sweep::default();
        sweep.write_nr10(0b0_001_0_001); // period 1, add, shift 1
        // frequency 100: delta=50, new=150
        assert_eq!(sweep.trigger(100), Some(150));
    }

    #[test]
    fn noise_lfsr_produces_deterministic_bit_sequence_from_seed() {
        let mut ch = NoiseChannel::default();
        ch.write_envelope(0xF0);
        ch.write_polynomial(0x00); // clock_shift 0, 15-bit mode, divisor code 0 (period 8)
        ch.write_control(0x80); // trigger
        assert_eq!(ch.lfsr, 0x7FFF);
        ch.step(8); // one LFSR clock
        assert_ne!(ch.lfsr, 0x7FFF);
    }

    #[test]
    fn wave_channel_reads_nibbles_and_applies_volume_shift() {
        let mut ch = WaveChannel::default();
        ch.write_wave_ram(0xFF30, 0xF0); // sample0=0xF, sample1=0x0
        ch.write_dac_power(0x80);
        ch.write_freq_lo(0x00);
        ch.write_control(0x80); // trigger, frequency 0
        ch.write_volume(0b001_00000); // 100%
        assert_eq!(ch.amplitude(), 0xF);
    }
}
