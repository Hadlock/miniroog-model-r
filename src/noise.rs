use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseColor {
    White,
    Pink,
    Brown,
    Blue,
    Violet,
    Grey,
}

impl NoiseColor {
    pub const VALUES: [NoiseColor; 6] = [
        NoiseColor::White,
        NoiseColor::Pink,
        NoiseColor::Brown,
        NoiseColor::Blue,
        NoiseColor::Violet,
        NoiseColor::Grey,
    ];

    pub const COUNT: usize = Self::VALUES.len();

    pub fn next(self) -> Self {
        let index = Self::VALUES
            .iter()
            .position(|color| *color == self)
            .unwrap_or(0);
        Self::VALUES[(index + 1) % Self::COUNT]
    }

    pub fn label(&self) -> &'static str {
        match self {
            NoiseColor::White => "WHITE",
            NoiseColor::Pink => "PINK",
            NoiseColor::Brown => "BROWN",
            NoiseColor::Blue => "BLUE",
            NoiseColor::Violet => "VIOLET",
            NoiseColor::Grey => "GREY",
        }
    }
}

#[derive(Clone)]
pub struct NoiseGenerator {
    seed: u64,
    pink: [f32; 7],
    brown: f32,
    white_last: f32,
    white_prev: f32,
}

impl NoiseGenerator {
    pub fn new() -> Self {
        Self::with_seed(random_seed())
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed: seed.max(1),
            pink: [0.0; 7],
            brown: 0.0,
            white_last: 0.0,
            white_prev: 0.0,
        }
    }

    pub fn sample(&mut self, color: NoiseColor) -> f32 {
        let white = self.white();
        let previous_last = self.white_last;
        let previous_prev = self.white_prev;
        self.white_prev = previous_last;
        self.white_last = white;
        let pink = self.pink_sample(white);
        let brown = self.brown_sample(white);
        let blue = (white - previous_last).clamp(-1.0, 1.0);
        let violet = (white - 2.0 * previous_last + previous_prev).clamp(-1.0, 1.0);
        let grey = (white * 0.35 + pink * 0.65).clamp(-1.0, 1.0);
        match color {
            NoiseColor::White => white,
            NoiseColor::Pink => pink,
            NoiseColor::Brown => brown,
            NoiseColor::Blue => blue,
            NoiseColor::Violet => violet,
            NoiseColor::Grey => grey,
        }
    }

    fn white(&mut self) -> f32 {
        // LCG: Numerical Recipes constants.
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let bits = self.seed >> 11;
        let normalized = (bits as f64) / ((1u64 << 53) as f64);
        (normalized as f32) * 2.0 - 1.0
    }

    fn pink_sample(&mut self, white: f32) -> f32 {
        self.pink[0] = 0.99886 * self.pink[0] + white * 0.0555179;
        self.pink[1] = 0.99332 * self.pink[1] + white * 0.0750759;
        self.pink[2] = 0.96900 * self.pink[2] + white * 0.1538520;
        self.pink[3] = 0.86650 * self.pink[3] + white * 0.3104856;
        self.pink[4] = 0.55000 * self.pink[4] + white * 0.5329522;
        self.pink[5] = -0.7616 * self.pink[5] - white * 0.0168980;
        self.pink[6] = white * 0.115926;
        (self.pink[0]
            + self.pink[1]
            + self.pink[2]
            + self.pink[3]
            + self.pink[4]
            + self.pink[5]
            + self.pink[6]
            + white * 0.5362)
            * 0.11
    }

    fn brown_sample(&mut self, white: f32) -> f32 {
        self.brown = (self.brown + white * 0.02).clamp(-1.5, 1.5);
        self.brown
    }
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_nanos() as u64)
        .unwrap_or(0x5EED)
}
