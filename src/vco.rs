use std::sync::{Arc, Mutex, mpsc};

use tokio::runtime::Runtime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Saw,
    Pulse,
    Triangle,
    Sine,
}

impl Waveform {
    pub fn label(&self) -> &'static str {
        match self {
            Waveform::Saw => "SAW",
            Waveform::Pulse => "PULSE",
            Waveform::Triangle => "TRI",
            Waveform::Sine => "SINE",
        }
    }

    pub fn sample(&self, phase: f32) -> f32 {
        match self {
            Waveform::Saw => 2.0 * (phase - 0.5),
            Waveform::Pulse => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            Waveform::Sine => (phase * std::f32::consts::TAU).sin(),
        }
    }
}

#[derive(Debug)]
pub struct VcoState {
    pub waveform: Waveform,
    pub voltage: f32,
    pub detune: f32,
    pub frequency: f32,
}

impl VcoState {
    pub fn new() -> Self {
        Self {
            waveform: Waveform::Saw,
            voltage: 0.0,
            detune: 0.0,
            frequency: voltage_to_frequency(0.0),
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    pub fn set_voltage(&mut self, voltage: f32) {
        self.voltage = voltage;
        self.frequency = voltage_to_frequency(self.voltage + self.detune);
    }

    pub fn set_detune(&mut self, detune: f32) {
        self.detune = detune;
        self.frequency = voltage_to_frequency(self.voltage + self.detune);
    }
}

#[derive(Debug)]
pub enum VcoCommand {
    SetVoltage(f32),
    SetDetune(f32),
    SetWaveform(Waveform),
}

pub type VcoHandle = (Arc<Mutex<VcoState>>, mpsc::Sender<VcoCommand>);

pub fn spawn_vco(runtime: &Runtime) -> VcoHandle {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(VcoState::new()));
    let thread_state = state.clone();

    runtime.spawn_blocking(move || {
        while let Ok(cmd) = rx.recv() {
            let mut guard = thread_state.lock().expect("lock VCO state");
            match cmd {
                VcoCommand::SetVoltage(voltage) => guard.set_voltage(voltage),
                VcoCommand::SetDetune(detune) => guard.set_detune(detune),
                VcoCommand::SetWaveform(waveform) => guard.set_waveform(waveform),
            }
        }
    });

    (state, tx)
}

const REFERENCE_FREQ: f32 = 55.0;

pub fn voltage_to_frequency(voltage: f32) -> f32 {
    let octave = voltage;
    REFERENCE_FREQ * 2.0f32.powf(octave)
}
