pub struct Mixer {
    levels: [f32; 3],
    pub master: f32,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            levels: [0.33; 3],
            master: 0.7,
        }
    }

    pub fn set_level(&mut self, index: usize, value: f32) {
        if let Some(level) = self.levels.get_mut(index) {
            *level = value.clamp(0.0, 1.0);
        }
    }

    pub fn mix(&self, oscillator_samples: &[f32]) -> f32 {
        oscillator_samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let level = self.levels.get(index).copied().unwrap_or(0.0);
                sample * level
            })
            .sum::<f32>()
            * self.master
    }
}
