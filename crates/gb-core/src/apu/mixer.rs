//! Mixes the four channels' current digital amplitudes (0-15 each) into a
//! stereo `f32` sample pair, honoring `NR51`'s per-channel left/right
//! panning and `NR50`'s per-side master volume (0-7).

/// Converts a 0-15 digital amplitude to a roughly [-1.0, 1.0] analog
/// sample. Real DAC output is bipolar around a nonzero baseline even for
/// a channel that's "on" but momentarily quiet (duty cycle low, etc), and
/// the real mixer has an analog high-pass filter that removes the
/// resulting DC bias over time; neither is modeled here. Instead,
/// amplitude 0 maps straight to silence (0.0) -- true for a disabled
/// channel, an approximation for an enabled-but-momentarily-0 one — which
/// avoids a disabled channel contributing an audible constant hum, the
/// practically-relevant case.
fn dac(amplitude: u8) -> f32 {
    if amplitude == 0 {
        0.0
    } else {
        (amplitude as f32 - 7.5) / 7.5
    }
}

/// Returns `(left, right)`, each summed from up to 4 channels then
/// scaled by that side's `NR50` volume (0-7) and normalized so a
/// fully-mixed max-volume signal stays within [-1.0, 1.0].
pub(super) fn mix(nr50: u8, nr51: u8, ch1: u8, ch2: u8, ch3: u8, ch4: u8) -> (f32, f32) {
    let left_vol = ((nr50 >> 4) & 0x07) as f32 / 7.0;
    let right_vol = (nr50 & 0x07) as f32 / 7.0;

    let amplitudes = [ch1, ch2, ch3, ch4];
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    for (i, &amp) in amplitudes.iter().enumerate() {
        let sample = dac(amp);
        if nr51 & (1 << (4 + i)) != 0 {
            left += sample;
        }
        if nr51 & (1 << i) != 0 {
            right += sample;
        }
    }

    // 4 channels max, each in [-1,1]: divide by 4 to keep the sum in
    // range before applying master volume.
    (left / 4.0 * left_vol, right / 4.0 * right_vol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_on_all_channels_mixes_to_zero() {
        let (l, r) = mix(0x77, 0xFF, 0, 0, 0, 0);
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn unrouted_channel_does_not_contribute() {
        // ch1 max amplitude, but NR51 routes nothing anywhere.
        let (l, r) = mix(0x77, 0x00, 15, 0, 0, 0);
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn master_volume_zero_silences_output() {
        let (l, r) = mix(0x00, 0xFF, 15, 15, 15, 15);
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn max_amplitude_all_channels_stays_within_unit_range() {
        let (l, r) = mix(0x77, 0xFF, 15, 15, 15, 15);
        assert!((-1.0..=1.0).contains(&l));
        assert!((-1.0..=1.0).contains(&r));
    }

    #[test]
    fn panning_routes_channel_to_only_the_selected_side() {
        // ch1 (amplitude 15) left-only (bit4), ch2 (amplitude 5) right-only (bit1).
        let (l, r) = mix(0x77, 0b0001_0010, 15, 5, 0, 0);
        assert_eq!(l, dac(15) / 4.0);
        assert_eq!(r, dac(5) / 4.0);
        assert_ne!(l, r);
    }
}
