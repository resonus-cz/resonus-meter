#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Platform audio sources and deterministic sources for Re:sonus Meter.

use std::{
    f64::consts::TAU,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(feature = "cpal-backend")]
use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::{Consumer, Producer, RingBuffer};

/// Metadata used to identify an input device independently from its UI index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable backend-specific device identifier.
    pub id: String,
    /// User-facing device or port name.
    pub display_name: String,
    /// Backend that exposed this device.
    pub backend: AudioBackend,
    /// Available input channel count.
    pub channels: u16,
    /// Supported sample rates in hertz.
    pub sample_rates_hz: Vec<u32>,
}

/// Audio backend associated with a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    /// PipeWire-native input.
    PipeWire,
    /// `PulseAudio` fallback input.
    PulseAudio,
    /// ALSA fallback input.
    Alsa,
    /// Windows Audio Session API input.
    Wasapi,
    /// A deterministic source for tests and the demo.
    Simulated,
}

/// The selected mono stream configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    /// Stable id returned by [`AudioSource::list_devices`].
    pub device_id: String,
    /// Zero-based selected channel.
    pub channel_index: u16,
    /// Requested stream sample rate in hertz.
    pub sample_rate_hz: u32,
    /// Requested buffer size in frames, or zero for the backend default.
    pub buffer_frames: u32,
}

/// Audio-source failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError {
    message: String,
}

impl AudioError {
    /// Creates an audio error with a user-facing explanation.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AudioError {}

/// Backend-neutral source of selected mono input samples.
pub trait AudioSource {
    /// Lists devices that can be opened by this source.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot enumerate its input devices.
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError>;

    /// Opens one selected channel and sends its samples to `sink`.
    ///
    /// # Errors
    ///
    /// Returns an error when the device cannot be opened with `config`.
    fn open(&mut self, config: StreamConfig, sink: SampleSink) -> Result<(), AudioError>;

    /// Stops and releases the active stream.
    fn close(&mut self);
}

/// cpal input source using `PipeWire` first, followed by `PulseAudio` and ALSA.
#[cfg(feature = "cpal-backend")]
pub struct CpalSource {
    host: cpal::Host,
    stream: Option<Stream>,
    stream_errors: Arc<AtomicU64>,
}

#[cfg(feature = "cpal-backend")]
impl Default for CpalSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "cpal-backend")]
impl CpalSource {
    /// Creates a source for cpal's preferred host.
    #[must_use]
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
            stream: None,
            stream_errors: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns the count of asynchronous stream errors since construction.
    #[must_use]
    pub fn stream_error_count(&self) -> u64 {
        self.stream_errors.load(Ordering::Relaxed)
    }

    fn device(&self, device_id: &str) -> Result<cpal::Device, AudioError> {
        self.host
            .input_devices()
            .map_err(|error| cpal_error(&error))?
            .find(|device| device.id().is_ok_and(|id| id.to_string() == device_id))
            .ok_or_else(|| AudioError::new("selected input device is not available"))
    }
}

#[cfg(feature = "cpal-backend")]
impl AudioSource for CpalSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        self.host
            .input_devices()
            .map_err(|error| cpal_error(&error))?
            .map(|device| {
                let id = device.id().map_err(|error| cpal_error(&error))?.to_string();
                let display_name = device.to_string();
                let configs = device
                    .supported_input_configs()
                    .map_err(|error| cpal_error(&error))?;
                let mut channels = 0;
                let mut sample_rates_hz = Vec::new();
                for config in configs {
                    if config.sample_format() != SampleFormat::F32 {
                        continue;
                    }
                    channels = channels.max(config.channels());
                    sample_rates_hz.push(config.min_sample_rate());
                    sample_rates_hz.push(config.max_sample_rate());
                    for rate in [44_100, 48_000, 96_000] {
                        if config.min_sample_rate() <= rate && rate <= config.max_sample_rate() {
                            sample_rates_hz.push(rate);
                        }
                    }
                }
                sample_rates_hz.sort_unstable();
                sample_rates_hz.dedup();
                Ok(DeviceInfo {
                    id,
                    display_name,
                    backend: audio_backend(self.host.id().name()),
                    channels,
                    sample_rates_hz,
                })
            })
            .collect()
    }

    fn open(&mut self, config: StreamConfig, mut sink: SampleSink) -> Result<(), AudioError> {
        self.close();
        let device = self.device(&config.device_id)?;
        let supported = device
            .supported_input_configs()
            .map_err(|error| cpal_error(&error))?
            .find(|candidate| {
                candidate.sample_format() == SampleFormat::F32
                    && candidate.channels() > config.channel_index
                    && candidate.min_sample_rate() <= config.sample_rate_hz
                    && candidate.max_sample_rate() >= config.sample_rate_hz
            })
            .ok_or_else(|| {
                AudioError::new("selected format is not supported by the input device")
            })?;
        let stream_config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: config.sample_rate_hz,
            buffer_size: match config.buffer_frames {
                0 => cpal::BufferSize::Default,
                frames => cpal::BufferSize::Fixed(frames),
            },
        };
        let channel_index = usize::from(config.channel_index);
        let channels = usize::from(stream_config.channels);
        let stream_errors = Arc::clone(&self.stream_errors);
        let stream = device
            .build_input_stream(
                stream_config,
                move |samples: &[f32], _| {
                    for frame in samples.chunks_exact(channels) {
                        sink.push_sample(frame[channel_index]);
                    }
                },
                move |_| {
                    stream_errors.fetch_add(1, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|error| cpal_error(&error))?;
        stream.play().map_err(|error| cpal_error(&error))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn close(&mut self) {
        self.stream = None;
    }
}

#[cfg(feature = "cpal-backend")]
fn cpal_error(error: &cpal::Error) -> AudioError {
    AudioError::new(error.to_string())
}

#[cfg(feature = "cpal-backend")]
fn audio_backend(name: &str) -> AudioBackend {
    match name {
        "pipewire" => AudioBackend::PipeWire,
        "pulseaudio" => AudioBackend::PulseAudio,
        _ => AudioBackend::Alsa,
    }
}

/// Audio-callback endpoint of a lock-free single-producer/single-consumer queue.
pub struct SampleSink {
    producer: Producer<f32>,
    dropped_samples: Arc<AtomicU64>,
}

impl SampleSink {
    /// Pushes one selected sample without allocating, locking, logging, or blocking.
    pub fn push_sample(&mut self, sample: f32) {
        if self.producer.push(sample).is_err() {
            self.dropped_samples.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Pushes a callback block without allocating, locking, logging, or blocking.
    pub fn push_block(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.push_sample(sample);
        }
    }
}

/// DSP-thread endpoint of a lock-free sample queue.
pub struct SampleReceiver {
    consumer: Consumer<f32>,
    dropped_samples: Arc<AtomicU64>,
}

impl SampleReceiver {
    /// Pops at most `samples.len()` queued samples into `samples`.
    pub fn pop_into(&mut self, samples: &mut [f32]) -> usize {
        let mut count = 0;
        for destination in samples {
            let Ok(sample) = self.consumer.pop() else {
                break;
            };
            *destination = sample;
            count += 1;
        }
        count
    }

    /// Returns the total callback samples dropped because the queue was full.
    #[must_use]
    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }
}

/// Creates the producer and consumer endpoints for a bounded mono sample queue.
#[must_use]
pub fn sample_queue(capacity: usize) -> (SampleSink, SampleReceiver) {
    let (producer, consumer) = RingBuffer::new(capacity);
    let dropped_samples = Arc::new(AtomicU64::new(0));
    (
        SampleSink {
            producer,
            dropped_samples: Arc::clone(&dropped_samples),
        },
        SampleReceiver {
            consumer,
            dropped_samples,
        },
    )
}

/// A deterministic mono sine-wave source for tests and the application demo.
#[derive(Debug, Clone)]
pub struct SineSource {
    frequency_hz: f64,
    sample_rate_hz: f64,
    amplitude: f32,
    phase_rad: f64,
}

impl SineSource {
    /// Creates a sine source with samples normalised to `amplitude`.
    ///
    /// # Panics
    ///
    /// Panics when the frequency or sample rate is not positive, or amplitude
    /// is outside the normalised `0.0..=1.0` range.
    #[must_use]
    pub fn new(frequency_hz: f64, sample_rate_hz: f64, amplitude: f32) -> Self {
        assert!(frequency_hz.is_sign_positive());
        assert!(sample_rate_hz.is_sign_positive());
        assert!((0.0..=1.0).contains(&amplitude));

        Self {
            frequency_hz,
            sample_rate_hz,
            amplitude,
            phase_rad: 0.0,
        }
    }

    /// Fills `samples` with successive source samples.
    pub fn fill(&mut self, samples: &mut [f32]) {
        for sample in samples {
            #[allow(clippy::cast_possible_truncation)]
            let sample_value = self.phase_rad.sin() as f32;
            *sample = sample_value * self.amplitude;
            self.phase_rad =
                (self.phase_rad + TAU * self.frequency_hz / self.sample_rate_hz).rem_euclid(TAU);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SineSource, sample_queue};

    #[test]
    fn produces_the_requested_amplitude() {
        let mut source = SineSource::new(1_000.0, 48_000.0, 0.5);
        let mut samples = [0.0; 48];
        source.fill(&mut samples);

        assert!((samples[12] - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn queue_reports_overruns_without_blocking_the_producer() {
        let (mut sink, mut receiver) = sample_queue(2);
        sink.push_block(&[0.1, 0.2, 0.3]);
        let mut samples = [0.0; 2];

        assert_eq!(receiver.pop_into(&mut samples), 2);
        assert!((samples[0] - 0.1).abs() < f32::EPSILON);
        assert!((samples[1] - 0.2).abs() < f32::EPSILON);
        assert_eq!(receiver.dropped_samples(), 1);
    }
}
