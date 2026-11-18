//! A Doppler schedule for satellite work: `--doppler FILE`, lines of `UNIX_SECONDS RX_HZ TX_HZ` (blank lines and
//! `#` comments ignored), as a pass predictor (gpredict, predict, a script) can write them.  Between lines the
//! shifts are interpolated linearly; before the first and after the last they hold.  They add to --rx-shift and
//! --tx-shift.
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    pts: Vec<(f64, f64, f64)>,
}

impl Schedule {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut pts = Vec::new();
        for (i, l) in text.lines().enumerate() {
            let l = l.split('#').next().unwrap_or("").trim();
            if l.is_empty() {
                continue;
            }
            let f: Vec<f64> = l.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|_| bad(i))?;
            match f[..] {
                [t, rx, tx] if f.iter().all(|v| v.is_finite()) => pts.push((t, rx, tx)),
                _ => return Err(bad(i)),
            }
        }
        if pts.is_empty() {
            return Err("no UNIX_SECONDS RX_HZ TX_HZ lines".into());
        }
        if pts.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err("times must increase".into());
        }
        Ok(Self { pts })
    }

    /// (rx_hz, tx_hz) at unix time `t`.
    pub fn at(&self, t: f64) -> (f64, f64) {
        let p = &self.pts;
        let i = p.partition_point(|q| q.0 <= t);
        if i == 0 {
            return (p[0].1, p[0].2);
        }
        if i == p.len() {
            return (p[i - 1].1, p[i - 1].2);
        }
        let (a, b) = (p[i - 1], p[i]);
        let f = (t - a.0) / (b.0 - a.0);
        (a.1 + f * (b.1 - a.1), a.2 + f * (b.2 - a.2))
    }

    pub fn now(&self) -> (f64, f64) {
        self.at(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()))
    }
}

fn bad(i: usize) -> String {
    format!("line {}: expected UNIX_SECONDS RX_HZ TX_HZ", i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_holds() {
        let s = Schedule::parse("# pass\n100 -50 40\n\n200 50 -40  # TCA\n300 60 -50\n").unwrap();
        assert_eq!(s.at(0.0), (-50.0, 40.0));
        assert_eq!(s.at(150.0), (0.0, 0.0));
        assert_eq!(s.at(250.0), (55.0, -45.0));
        assert_eq!(s.at(1e9), (60.0, -50.0));
        for bad in ["", "# x", "1 2", "1 2 3 4", "1 a 3", "2 0 0\n1 0 0", "1 inf 0"] {
            assert!(Schedule::parse(bad).is_err(), "{bad:?}");
        }
    }
}
