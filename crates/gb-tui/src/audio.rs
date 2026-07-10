//! `cpal` audio output: pulls interleaved stereo `f32` samples from the
//! `gb_core::apu::Apu`'s ring buffer consumer and feeds them to the
//! default output device. This is the one place in `gb-tui` that touches
//! `cpal` — `gb-core` only knows about the generic ring buffer, per the
//! crate boundary described in `apu/mod.rs`.
//!
//! Audio is a nice-to-have: any failure here (no device, unsupported
//! config, stream build failure) is logged and degrades to running
//! silently rather than propagated as a hard error, since none of it
//! should prevent the emulator from starting.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use ringbuf::traits::Consumer;
use ringbuf::HeapCons;

/// Owns the live stream (dropping it stops playback) and the sample rate
/// it negotiated with the device — feed that back into
/// [`gb_core::apu::Apu::set_sample_rate`] so emulation produces samples at
/// the rate actually being consumed.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub sample_rate: u32,
}

/// Starts a `cpal` output stream pulling samples from `consumer`. Returns
/// `None` (after logging why) if no output device is available or the
/// stream can't be built.
pub fn start(mut consumer: HeapCons<f32>) -> Option<AudioOutput> {
    let host = cpal::default_host();
    let device = host.default_output_device().or_else(|| {
        tracing::warn!("no audio output device available; running without sound");
        None
    })?;

    let supported_config = device
        .default_output_config()
        .inspect_err(|e| tracing::warn!("failed to query audio output config: {e}; running without sound"))
        .ok()?;

    let sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels();
    let config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                fill_output(data, channels, &mut consumer);
            },
            |err| tracing::warn!("audio stream error: {err}"),
            None,
        )
        .inspect_err(|e| tracing::warn!("failed to build audio output stream: {e}; running without sound"))
        .ok()?;

    if let Err(e) = stream.play() {
        tracing::warn!("failed to start audio output stream: {e}; running without sound");
        return None;
    }

    tracing::info!("audio output started at {sample_rate} Hz, {channels} channel(s)");
    Some(AudioOutput { _stream: stream, sample_rate })
}

/// Fills `data` (interleaved frames, `channels` samples each) from the
/// emulator's stereo ring buffer. Downmixes to mono or pads to more
/// channels as needed, and fills with silence on an underrun rather than
/// stalling the audio thread.
fn fill_output(data: &mut [f32], channels: u16, consumer: &mut HeapCons<f32>) {
    let channels = channels.max(1) as usize;
    for frame in data.chunks_mut(channels) {
        let left = consumer.try_pop().unwrap_or(0.0);
        let right = consumer.try_pop().unwrap_or(0.0);
        if channels == 1 {
            frame[0] = (left + right) / 2.0;
        } else {
            frame[0] = left;
            frame[1] = right;
            for sample in frame.iter_mut().skip(2) {
                *sample = 0.0;
            }
        }
    }
}
