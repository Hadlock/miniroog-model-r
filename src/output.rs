use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, Stream,
};

use crate::{mixer::Mixer, modifiers::Modifiers, oscillatorbank::OscillatorBank};

pub type SharedPipeline = Arc<Mutex<SynthPipeline>>;
pub type DebugHandle = Arc<Mutex<DebugData>>;

pub struct SynthPipeline {
    bank: OscillatorBank,
    mixer: Mixer,
    modifiers: Modifiers,
    sample_rate: f32,
    voice_buffer: Vec<f32>,
}

impl SynthPipeline {
    pub fn new(bank: OscillatorBank, mixer: Mixer, modifiers: Modifiers) -> Self {
        let voice_buffer = vec![0.0; bank.len()];
        Self {
            bank,
            mixer,
            modifiers,
            sample_rate: 44_100.0,
            voice_buffer,
        }
    }

    pub fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate.max(1.0);
    }

    pub fn set_gate(&mut self, gate: bool) {
        self.modifiers.set_gate(gate);
    }

    pub fn set_mix_level(&mut self, index: usize, level: f32) {
        self.mixer.set_level(index, level);
    }

    pub fn set_master_level(&mut self, value: f32) {
        self.mixer.master = value.clamp(0.0, 1.0);
    }

    pub fn set_cutoff(&mut self, hz: f32) {
        self.modifiers.set_cutoff(hz);
    }

    pub fn next_sample(&mut self) -> f32 {
        self.bank
            .fill_sample(self.sample_rate, &mut self.voice_buffer);
        let mixed = self.mixer.mix(&self.voice_buffer);
        self.modifiers
            .process(mixed, 1.0 / self.sample_rate.max(1.0))
    }
}

pub struct DebugData {
    buffer: Vec<f32>,
    cursor: usize,
    filled: bool,
}

impl DebugData {
    pub fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            cursor: 0,
            filled: false,
        }
    }

    pub fn push(&mut self, value: f32) {
        if let Some(slot) = self.buffer.get_mut(self.cursor) {
            *slot = value;
        }
        self.cursor = (self.cursor + 1) % self.buffer.len();
        if self.cursor == 0 {
            self.filled = true;
        }
    }

    pub fn snapshot(&self) -> Vec<f32> {
        if !self.filled {
            return self.buffer[..self.cursor].to_vec();
        }
        let mut data = Vec::with_capacity(self.buffer.len());
        data.extend_from_slice(&self.buffer[self.cursor..]);
        data.extend_from_slice(&self.buffer[..self.cursor]);
        data
    }
}

pub struct AudioEngine {
    _stream: Stream,
}

impl AudioEngine {
    pub fn start(pipeline: SharedPipeline, debug: DebugHandle) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default audio output"))?;
        let supported = device.default_output_config()?;
        let config = supported.config();
        let sample_rate = config.sample_rate.0 as f32;
        {
            let mut guard = pipeline.lock().expect("pipeline lock");
            guard.set_sample_rate(sample_rate);
        }
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_stream_f32(&device, &config, pipeline, debug)?,
            SampleFormat::I16 => build_stream_i16(&device, &config, pipeline, debug)?,
            SampleFormat::U16 => build_stream_u16(&device, &config, pipeline, debug)?,
            _ => build_stream_f32(&device, &config, pipeline, debug)?,
        };
        stream.play()?;
        Ok(Self { _stream: stream })
    }
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pipeline: SharedPipeline,
    debug: DebugHandle,
) -> Result<Stream> {
    let channels = config.channels as usize;
    let config = config.clone();
    let stream = device.build_output_stream(
        &config,
        move |output: &mut [f32], _| {
            fill_output_buffer(output, channels, &pipeline, &debug, |sample| sample);
        },
        move |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pipeline: SharedPipeline,
    debug: DebugHandle,
) -> Result<Stream> {
    let channels = config.channels as usize;
    let config = config.clone();
    let stream = device.build_output_stream(
        &config,
        move |output: &mut [i16], _| {
            fill_output_buffer(output, channels, &pipeline, &debug, |sample| {
                (sample * i16::MAX as f32) as i16
            });
        },
        move |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pipeline: SharedPipeline,
    debug: DebugHandle,
) -> Result<Stream> {
    let channels = config.channels as usize;
    let config = config.clone();
    let stream = device.build_output_stream(
        &config,
        move |output: &mut [u16], _| {
            fill_output_buffer(output, channels, &pipeline, &debug, |sample| {
                let scaled = (sample * 0.5 + 0.5).clamp(0.0, 1.0);
                (scaled * u16::MAX as f32) as u16
            });
        },
        move |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

fn fill_output_buffer<T, F>(
    output: &mut [T],
    channels: usize,
    pipeline: &SharedPipeline,
    debug: &DebugHandle,
    mut convert: F,
) where
    F: FnMut(f32) -> T,
    T: Copy,
{
    let mut pipe = pipeline.lock().expect("pipeline lock");
    let mut debug_guard = debug.lock().expect("debug lock");
    for frame in output.chunks_mut(channels) {
        let sample = pipe.next_sample().clamp(-0.98, 0.98);
        debug_guard.push(sample);
        let value = convert(sample);
        for channel in frame {
            *channel = value;
        }
    }
}
