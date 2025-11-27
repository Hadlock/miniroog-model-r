use std::f32::consts::PI;

use rustfft::{FftPlanner, num_complex::Complex};

const FILTER_MIN_CUTOFF: f32 = 80.0;
const FILTER_MAX_CUTOFF: f32 = 18_000.0;
const FILTER_CONTOUR_DEPTH: f32 = 4.0;

pub struct Modifiers {
    gate_open: bool,
    cutoff_hz: f32,
    emphasis: f32,
    contour_amount: f32,
    filter_params: EnvelopeParams,
    loud_params: EnvelopeParams,
    filter_env: AdsrEnvelope,
    loud_env: AdsrEnvelope,
    ladder: LadderFilter,
}

impl Modifiers {
    pub fn new() -> Self {
        Self {
            gate_open: false,
            cutoff_hz: 2_000.0,
            emphasis: 0.0,
            contour_amount: 0.0,
            filter_params: EnvelopeParams::default(),
            loud_params: EnvelopeParams::default(),
            filter_env: AdsrEnvelope::new(),
            loud_env: AdsrEnvelope::new(),
            ladder: LadderFilter::new(),
        }
    }

    pub fn set_gate(&mut self, gate: bool) {
        if gate && !self.gate_open {
            self.filter_env.trigger();
            self.loud_env.trigger();
        } else if !gate && self.gate_open {
            self.filter_env.release();
            self.loud_env.release();
        }
        self.gate_open = gate;
    }

    pub fn set_cutoff(&mut self, hz: f32) {
        self.cutoff_hz = hz.clamp(FILTER_MIN_CUTOFF, FILTER_MAX_CUTOFF);
    }

    pub fn set_emphasis(&mut self, value: f32) {
        self.emphasis = value.clamp(0.0, 1.0);
    }

    pub fn set_contour_amount(&mut self, value: f32) {
        self.contour_amount = value.clamp(0.0, 1.0);
    }

    pub fn set_filter_envelope(&mut self, attack: f32, decay: f32, sustain: f32) {
        self.filter_params = EnvelopeParams {
            attack: map_env_time(attack, 0.0015, 3.0),
            decay: map_env_time(decay, 0.005, 4.0),
            sustain: sustain.clamp(0.0, 1.0),
            release: map_env_time(decay, 0.005, 4.0),
        };
    }

    pub fn set_loudness_envelope(&mut self, attack: f32, decay: f32, sustain: f32) {
        self.loud_params = EnvelopeParams {
            attack: map_env_time(attack, 0.001, 4.5),
            decay: map_env_time(decay, 0.01, 6.0),
            sustain: sustain.clamp(0.0, 1.0),
            release: map_env_time(decay, 0.01, 6.0),
        };
    }

    pub fn process(&mut self, input: f32, dt: f32) -> f32 {
        let filter_env = self.filter_env.advance(dt, &self.filter_params);
        let loud_env = self.loud_env.advance(dt, &self.loud_params);

        let contour_scale = 1.0 + self.contour_amount * filter_env * FILTER_CONTOUR_DEPTH;
        let dynamic_cutoff =
            (self.cutoff_hz * contour_scale).clamp(FILTER_MIN_CUTOFF, FILTER_MAX_CUTOFF);
        let filtered = self
            .ladder
            .process(input, dynamic_cutoff, self.emphasis, dt);

        filtered * loud_env
    }
}

pub fn compute_spectrum(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let size = samples.len().next_power_of_two().max(8);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut buffer = vec![Complex::new(0.0, 0.0); size];
    for (idx, value) in samples.iter().enumerate().take(size) {
        buffer[idx].re = *value;
    }
    fft.process(&mut buffer);
    buffer[..size / 2]
        .iter()
        .map(|c| c.norm() / size as f32)
        .collect()
}

#[derive(Clone, Copy)]
struct EnvelopeParams {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Default for EnvelopeParams {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.2,
        }
    }
}

#[derive(Clone, Copy)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

struct AdsrEnvelope {
    value: f32,
    stage: EnvStage,
}

impl AdsrEnvelope {
    fn new() -> Self {
        Self {
            value: 0.0,
            stage: EnvStage::Idle,
        }
    }

    fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
    }

    fn release(&mut self) {
        if !matches!(self.stage, EnvStage::Idle) {
            self.stage = EnvStage::Release;
        }
    }

    fn advance(&mut self, dt: f32, params: &EnvelopeParams) -> f32 {
        match self.stage {
            EnvStage::Idle => {
                self.value = 0.0;
            }
            EnvStage::Attack => {
                let step = dt / params.attack.max(0.0001);
                self.value += (1.0 - self.value) * step;
                if (1.0 - self.value).abs() < 0.001 {
                    self.value = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                let step = dt / params.decay.max(0.0001);
                self.value += (params.sustain - self.value) * step;
                if (self.value - params.sustain).abs() < 0.001 {
                    self.value = params.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => {
                self.value = params.sustain;
            }
            EnvStage::Release => {
                let step = dt / params.release.max(0.0001);
                self.value += (0.0 - self.value) * step;
                if self.value <= 0.0001 {
                    self.value = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.value.clamp(0.0, 1.0)
    }
}

struct LadderFilter {
    stage: [f32; 4],
}

impl LadderFilter {
    fn new() -> Self {
        Self { stage: [0.0; 4] }
    }

    fn process(&mut self, input: f32, cutoff: f32, emphasis: f32, dt: f32) -> f32 {
        let g = (2.0 * PI * cutoff * dt).clamp(0.0, 0.99);
        let resonance = emphasis.clamp(0.0, 1.0) * 4.0;

        let feedback = self.stage[3] * resonance;
        let drive = (input - feedback).tanh();

        self.stage[0] += g * (drive - self.stage[0]);
        self.stage[1] += g * (self.stage[0].tanh() - self.stage[1]);
        self.stage[2] += g * (self.stage[1].tanh() - self.stage[2]);
        self.stage[3] += g * (self.stage[2].tanh() - self.stage[3]);

        self.stage[3]
    }
}

fn map_env_time(value: f32, min: f32, max: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    let ratio = max / min;
    min * ratio.powf(clamped)
}
