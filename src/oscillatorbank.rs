use std::sync::{Arc, Mutex};

use crate::vco::VcoState;

pub struct OscillatorVoice {
    state: Arc<Mutex<VcoState>>,
    phase: f32,
}

impl OscillatorVoice {
    fn new(state: Arc<Mutex<VcoState>>) -> Self {
        Self { state, phase: 0.0 }
    }

    fn sample(&mut self, sample_rate: f32) -> f32 {
        let (frequency, waveform) = {
            let guard = self.state.lock().expect("lock voice");
            (guard.frequency, guard.waveform)
        };
        let phase_delta = frequency / sample_rate;
        self.phase = (self.phase + phase_delta).fract();
        waveform.sample(self.phase)
    }
}

pub struct OscillatorBank {
    voices: Vec<OscillatorVoice>,
}

impl OscillatorBank {
    pub fn new(states: Vec<Arc<Mutex<VcoState>>>) -> Self {
        let voices = states.into_iter().map(OscillatorVoice::new).collect();
        Self { voices }
    }

    pub fn len(&self) -> usize {
        self.voices.len()
    }

    pub fn fill_sample(&mut self, sample_rate: f32, out: &mut [f32]) {
        for (index, voice) in self.voices.iter_mut().enumerate() {
            if let Some(slot) = out.get_mut(index) {
                *slot = voice.sample(sample_rate);
            }
        }
    }
}
