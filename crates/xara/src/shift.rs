//! Audio frequency shift, as VARA SAT's "TX Shift" / "RX Shift" (e.g. -1000 Hz) for satellite
//! Doppler and transponder offsets: real audio -> analytic signal (FIR Hilbert) -> mixed by
//! e^(j2πft) -> real.  A positive shift moves the audio up.
use std::f64::consts::PI;

const TAPS: usize = 255;
const MID: usize = TAPS / 2;
pub const MAX_HZ: f64 = 2000.0;

pub struct Shift {
    step: f64,
    phase: f64,
    h: Vec<(usize, f64)>,
    ring: Vec<f64>,
    pos: usize,
}

impl Shift {
    /// `None` when `hz` is 0, so an unshifted path costs nothing and adds no delay.
    pub fn new(hz: f64, fs: f64) -> Option<Self> {
        if hz == 0.0 {
            return None;
        }
        // odd taps only; Blackman window keeps the image of a 300-3000 Hz signal below about -50 dB
        let h = (0..TAPS)
            .filter(|k| (k + MID) % 2 == 1)
            .map(|k| {
                let m = k as f64 - MID as f64;
                let w = 0.42 - 0.5 * (2.0 * PI * k as f64 / (TAPS - 1) as f64).cos()
                    + 0.08 * (4.0 * PI * k as f64 / (TAPS - 1) as f64).cos();
                (k, 2.0 / (PI * m) * w)
            })
            .collect();
        Some(Self { step: 2.0 * PI * hz / fs, phase: 0.0, h, ring: vec![0.0; TAPS], pos: 0 })
    }

    pub fn process(&mut self, x: &mut [f64]) {
        for v in x.iter_mut() {
            self.ring[self.pos] = *v;
            // ring[(pos - k) mod TAPS] is x[n - k]
            let at = |k: usize| self.ring[(self.pos + TAPS - k) % TAPS];
            let im: f64 = self.h.iter().map(|&(k, c)| c * at(k)).sum();
            let re = at(MID);
            *v = re * self.phase.cos() - im * self.phase.sin();
            self.phase = (self.phase + self.step) % (2.0 * PI);
            self.pos = (self.pos + 1) % TAPS;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power(x: &[f64], hz: f64) -> f64 {
        let w = 2.0 * PI * hz / 48000.0;
        let (c, s) = x
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(c, s), (n, v)| (c + v * (w * n as f64).cos(), s + v * (w * n as f64).sin()));
        (c * c + s * s) / (x.len() * x.len()) as f64
    }

    #[test]
    fn moves_a_tone_and_suppresses_the_image() {
        for (tone, shift) in [(1500.0, 200.0), (1500.0, -1000.0), (800.0, 140.0), (2400.0, -140.0)] {
            let mut x: Vec<f64> = (0..48000).map(|n| (2.0 * PI * tone * n as f64 / 48000.0).sin()).collect();
            let mut s = Shift::new(shift, 48000.0).unwrap();
            for c in x.chunks_mut(480) {
                s.process(c);
            }
            let y = &x[TAPS..];
            let (want, image) = (power(y, tone + shift), power(y, tone - shift));
            assert!((want - 0.25).abs() < 0.01, "{tone}{shift:+}: power {want}");
            assert!(10.0 * (image / want).log10() < -45.0, "{tone}{shift:+}: image {image}");
        }
        assert!(Shift::new(0.0, 48000.0).is_none());
    }
}
