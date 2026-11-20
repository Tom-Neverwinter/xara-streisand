pub mod ctrl;
pub mod frame;
pub mod fsk;
pub mod level;
pub mod ofdm;
pub mod psk;
pub mod rx;
pub mod tx;

use crate::dat::{RecordTables, hf_tables, parse_hf_dat};
use crate::seq;
use level::Bandwidth;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::path::Path;
use std::sync::{LazyLock, OnceLock};

type Phasors = Vec<Vec<Complex64>>;

static TABLES: [OnceLock<Vec<RecordTables>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
static WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::HF_SEED));
static SAT_WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::SAT_SEED));
static SAT_PSK_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::psk_phasor_index(seq::SAT_SEED)));
static SAT_OFDM_PHASORS: LazyLock<Phasors> = LazyLock::new(|| phasors(seq::ofdm_phasor_index(seq::SAT_SEED)));
static PSK_PHASORS: OnceLock<Phasors> = OnceLock::new();
static OFDM_PHASORS: OnceLock<Phasors> = OnceLock::new();

/// VARA HF 4.9.1's table set for 2027 on (it switches on `Year(Now) >= 2027`).  Its tables are
/// not generated from a seed, so they are read from a VARA install rather than shipped.
struct Set2027 {
    tables: [Vec<RecordTables>; 3],
    psk: Phasors,
    ofdm: Phasors,
}
static SET_2027: OnceLock<Set2027> = OnceLock::new();

/// Switch this process to the 2027 table set, read from VARA HF 4.9.1's `Mod1.dat`, `Mod2.dat`
/// and `Binary1-3.dat` in `vara_dir`.  Call once at startup, before anything uses the tables.
pub fn use_2027_tables(vara_dir: &Path) -> Result<(), String> {
    if TABLES.iter().any(|t| t.get().is_some()) || PSK_PHASORS.get().is_some() || OFDM_PHASORS.get().is_some() {
        return Err("2027 tables must be selected before the legacy tables are used".into());
    }
    let read = |name: &str, len: Option<usize>| -> Result<Vec<u8>, String> {
        let p = vara_dir.join(name);
        let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        match len {
            Some(n) if b.len() != n => Err(format!("{}: {} bytes, expected {n}", p.display(), b.len())),
            _ => Ok(b),
        }
    };
    let ofdm = read("Mod1.dat", Some(seq::OFDM_BINS * seq::OFDM_SYMBOLS))?;
    let psk = read("Mod2.dat", Some(seq::PSK_CARRIERS * seq::PSK_SYMBOLS))?;
    let mut tables: [Vec<RecordTables>; 3] = Default::default();
    for (bw, file) in
        [(Bandwidth::Hz500, "Binary1.dat"), (Bandwidth::Hz2300, "Binary2.dat"), (Bandwidth::Hz2750, "Binary3.dat")]
    {
        tables[bw as usize] = parse_hf_dat(bw, &read(file, None)?, true).map_err(|e| format!("{file}: {e}"))?;
    }
    let set = Set2027 {
        tables,
        psk: phasors(seq::psk_layout(psk.into_iter())),
        ofdm: phasors(seq::ofdm_layout(ofdm.into_iter())),
    };
    SET_2027.set(set).map_err(|_| "2027 tables already selected".to_string())
}

/// True once `use_2027_tables` has switched this process to the 2027 set.
pub fn uses_2027_tables() -> bool {
    SET_2027.get().is_some()
}

fn phasors<const N: usize>(idx: Vec<[u8; N]>) -> Phasors {
    idx.iter()
        .map(|r| r.iter().map(|&d| Complex64::from_polar(1.0, 2.0 * PI * f64::from(d) / 128.0)).collect())
        .collect()
}

pub fn tables(bw: Bandwidth) -> &'static [RecordTables] {
    match SET_2027.get() {
        Some(s) if bw != Bandwidth::Sat => &s.tables[bw as usize],
        _ => TABLES[bw as usize].get_or_init(|| hf_tables(bw)),
    }
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

/// VARA SAT draws its PSK training symbols from its whitening stream (seed 414), as HF does from 292; the 2027
/// set (VARA HF 4.9.1) replaces HF's, not SAT's.
pub fn psk_phasors(bw: Bandwidth) -> &'static [Vec<Complex64>] {
    match SET_2027.get() {
        _ if bw == Bandwidth::Sat => &SAT_PSK_PHASORS,
        Some(s) => &s.psk,
        None => PSK_PHASORS.get_or_init(|| phasors(seq::psk_phasor_index(seq::HF_SEED))),
    }
}

/// SAT's OFDM training symbols, from seed 414 as its PSK ones are (by analogy; not yet seen on air).
pub fn ofdm_phasors(bw: Bandwidth) -> &'static [Vec<Complex64>] {
    match SET_2027.get() {
        _ if bw == Bandwidth::Sat => &SAT_OFDM_PHASORS,
        Some(s) => &s.ofdm,
        None => OFDM_PHASORS.get_or_init(|| phasors(seq::ofdm_phasor_index(seq::HF_SEED))),
    }
}
