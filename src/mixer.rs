pub struct Mixer {
    levels: [f32; 3],
    osc_enabled: [bool; 3],
    noise_level: f32,
    noise_enabled: bool,
    pub master: f32,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            levels: [0.33; 3],
            osc_enabled: [true; 3],
            noise_level: 0.0,
            noise_enabled: true,
            master: 0.7,
        }
    }

    pub fn set_level(&mut self, index: usize, value: f32) {
        if let Some(level) = self.levels.get_mut(index) {
            *level = value.clamp(0.0, 1.0);
        }
    }

    pub fn set_osc_enabled(&mut self, index: usize, enabled: bool) {
        if let Some(slot) = self.osc_enabled.get_mut(index) {
            *slot = enabled;
        }
    }

    pub fn set_noise_level(&mut self, value: f32) {
        self.noise_level = value.clamp(0.0, 1.0);
    }

    pub fn set_noise_enabled(&mut self, enabled: bool) {
        self.noise_enabled = enabled;
    }

    pub fn mix(&self, oscillator_samples: &[f32], noise_sample: f32) -> f32 {
        let oscillators = oscillator_samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                if self.osc_enabled.get(index).copied().unwrap_or(false) {
                    let level = self.levels.get(index).copied().unwrap_or(0.0);
                    sample * level
                } else {
                    0.0
                }
            })
            .sum::<f32>();
        let noise = if self.noise_enabled {
            noise_sample * self.noise_level
        } else {
            0.0
        };
        (oscillators + noise) * self.master
    }
}
