use std::f64::consts::TAU;

pub(super) struct Draw(u64);

impl Draw {
    pub(super) fn new(key: &str) -> Self {
        Self(key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3)))
    }

    fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        (((z ^ (z >> 31)) >> 12) as f64 + 0.5) / (1u64 << 52) as f64
    }

    fn gamma(&mut self, shape: f64) -> f64 {
        if shape < 1.0 {
            return self.gamma(shape + 1.0) * self.uniform().powf(1.0 / shape);
        }
        let d = shape - 1.0 / 3.0;
        let c = (9.0 * d).sqrt().recip();
        loop {
            let x = (-2.0 * self.uniform().ln()).sqrt() * (TAU * self.uniform()).cos();
            let v = (1.0 + c * x).powi(3);
            if v <= 0.0 {
                continue;
            }
            let u = self.uniform();
            if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
                return d * v;
            }
        }
    }

    pub(super) fn beta(&mut self, mean: f64, weight: f64) -> f64 {
        let a = self.gamma(mean * weight);
        let b = self.gamma((1.0 - mean) * weight);
        a / (a + b)
    }
}
