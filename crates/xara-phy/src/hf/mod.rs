pub mod ctrl;
pub mod frame;
pub mod fsk;
pub mod level;
pub mod ofdm;
pub mod psk;
pub mod rx;
pub mod tx;

use crate::dat::{RecordTables, hf_tables};
use crate::seq;
use level::Bandwidth;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::sync::{LazyLock, OnceLock};

type Phasors = Vec<Vec<Complex64>>;

static TABLES: [OnceLock<Vec<RecordTables>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
static WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::HF_SEED));
static SAT_WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::SAT_SEED));
static PSK_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::psk_phasor_index(seq::HF_SEED)));
static SAT_PSK_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::psk_phasor_index(seq::SAT_SEED)));
static OFDM_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::ofdm_phasor_index(seq::HF_SEED)));
static SAT_OFDM_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::ofdm_phasor_index(seq::SAT_SEED)));

fn phasors<const N: usize>(idx: Vec<[u8; N]>) -> Phasors {
    idx.iter()
        .map(|r| r.iter().map(|&d| Complex64::from_polar(1.0, 2.0 * PI * f64::from(d) / 128.0)).collect())
        .collect()
}

pub fn tables(bw: Bandwidth) -> &'static [RecordTables] {
    TABLES[bw as usize].get_or_init(|| hf_tables(bw))
}

/// VARA SAT whitens its data frames with its own Rnd stream (seed 414; lab recordings, 2026-11-18).
pub fn whitening(bw: Bandwidth) -> &'static [u8] {
    if bw == Bandwidth::Sat { &SAT_WHITE } else { &WHITE }
}

pub fn warm(bw: Bandwidth) {
    tables(bw);
    whitening(bw);
    psk_phasors(bw);
    ofdm_phasors(bw);
}

/// VARA SAT draws its PSK training symbols from its whitening stream (seed 414), as HF does from 292.
pub fn psk_phasors(bw: Bandwidth) -> &'static [Vec<Complex64>] {
    if bw == Bandwidth::Sat { &SAT_PSK_PHASORS } else { &PSK_PHASORS }
}

/// SAT's OFDM training symbols, from seed 414 as its PSK ones are (by analogy; not yet seen on air).
pub fn ofdm_phasors(bw: Bandwidth) -> &'static [Vec<Complex64>] {
    if bw == Bandwidth::Sat { &SAT_OFDM_PHASORS } else { &OFDM_PHASORS }
}
