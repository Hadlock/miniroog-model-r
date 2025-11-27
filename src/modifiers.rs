use rustfft::{FftPlanner, num_complex::Complex};

pub struct Modifiers {
    gate_open: bool,
    envelope: f32,
    filter_state: f32,
    pub attack: f32,
    pub release: f32,
    pub cutoff_hz: f32,
}

impl Modifiers {
    pub fn new() -> Self {
        Self {
            gate_open: false,
            envelope: 0.0,
            filter_state: 0.0,
            attack: 0.02,
            release: 0.2,
            cutoff_hz: 2_000.0,
        }
    }

    pub fn set_gate(&mut self, gate: bool) {
        self.gate_open = gate;
    }

    pub fn set_cutoff(&mut self, hz: f32) {
        self.cutoff_hz = hz.max(80.0);
    }

    pub fn process(&mut self, input: f32, dt: f32) -> f32 {
        let target = if self.gate_open { 1.0 } else { 0.0 };
        let rate = if self.gate_open {
            (dt / self.attack.max(0.0001)).min(1.0)
        } else {
            (dt / self.release.max(0.0001)).min(1.0)
        };
        self.envelope += (target - self.envelope) * rate;
        let env_applied = input * self.envelope;
        let rc = (1.0 / (self.cutoff_hz * std::f32::consts::TAU)).max(0.00001);
        let alpha = dt / (rc + dt);
        self.filter_state += alpha * (env_applied - self.filter_state);
        self.filter_state
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
