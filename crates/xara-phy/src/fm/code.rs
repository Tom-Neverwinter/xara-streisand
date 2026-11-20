use super::level::{Level, Record};
use crate::dat::{RecordTables, fm_tables};
use crate::decode::{FrameOut, decode_frame};
use crate::fec::{frame_bits, interleave, turbo_encode, whiten};
use crate::fm::level::FmBandwidth;
use crate::rnd::VbRnd;
use crate::seq;
use crate::turbo::Decoder;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

pub const PHASOR_ROWS: usize = 201;
pub const BINS: usize = 128;

pub type Phasors = Vec<[Complex64; PHASOR_ROWS]>;

static NARROW: OnceLock<Vec<RecordTables>> = OnceLock::new();
static WIDE: OnceLock<Vec<RecordTables>> = OnceLock::new();
static WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::FM_SEED));

static FM_2027: OnceLock<[Vec<RecordTables>; 2]> = OnceLock::new();

/// Switch this process to VARA FM 4.4.1's 2027 table set, read from `Binary1.dat` (narrow) and
/// `Binary2.dat` (wide) in `vara_dir`.  Its tables are not generated from a seed.  Call once at
/// startup, before anything uses the tables.  The FM phase scramble is left as is: no date gate
/// on it has been found.
/// True once `use_2027_tables` has switched this process to the 2027 set.
pub fn uses_2027_tables() -> bool {
    FM_2027.get().is_some()
}

pub fn use_2027_tables(vara_dir: &std::path::Path) -> Result<(), String> {
    if NARROW.get().is_some() || WIDE.get().is_some() {
        return Err("2027 tables must be selected before the legacy tables are used".into());
    }
    let load = |bw, name: &str| {
        let p = vara_dir.join(name);
        let raw = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        crate::dat::parse_fm_dat(bw, &raw, true).map_err(|e| format!("{name}: {e}"))
    };
    let set = [load(FmBandwidth::Narrow, "Binary1.dat")?, load(FmBandwidth::Wide, "Binary2.dat")?];
    FM_2027.set(set).map_err(|_| "2027 tables already selected".to_string())
}

pub fn tables(bw: FmBandwidth) -> &'static [RecordTables] {
    if let Some(s) = FM_2027.get() {
        return &s[bw as usize];
    }
    match bw {
        FmBandwidth::Narrow => NARROW.get_or_init(|| fm_tables(bw)),
        FmBandwidth::Wide => WIDE.get_or_init(|| fm_tables(bw)),
    }
}

pub fn whitening() -> &'static [u8] {
    &WHITE
}

pub fn warm() {
    for bw in [FmBandwidth::Narrow, FmBandwidth::Wide] {
        tables(bw);
    }
    whitening();
}

pub fn phase_indices(key: Option<u32>) -> Vec<[u8; PHASOR_ROWS]> {
    let mut r = match key {
        None => VbRnd::stream(seq::FM_SEED),
        Some(k) => {
            let mut r = VbRnd::new();
            r.seed_rnd(k);
            r
        }
    };
    (0..BINS).map(|_| std::array::from_fn(|_| r.rand_int(32) as u8)).collect()
}

pub fn phasors(key: Option<u32>) -> Arc<Phasors> {
    static CACHE: Mutex<Vec<(Option<u32>, Arc<Phasors>)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap();
    if let Some((_, p)) = c.iter().find(|(k, _)| *k == key) {
        return p.clone();
    }
    let p: Arc<Phasors> = Arc::new(
        phase_indices(key).iter().map(|r| r.map(|d| Complex64::from_polar(1.0, PI * f64::from(d) / 15.0))).collect(),
    );
    if c.len() >= 8 {
        c.remove(0);
    }
    c.push((key, p.clone()));
    p
}

pub const fn record_key(record: Record, key: u32) -> Option<u32> {
    if record.keyed() { Some(key) } else { None }
}

pub struct Codec {
    pub bw: FmBandwidth,
    turbo: Decoder,
    pub early: bool,
}

impl Codec {
    pub fn new(bw: FmBandwidth) -> Self {
        Self { bw, turbo: Decoder::default(), early: true }
    }

    pub fn level(&self, record: Record) -> Level {
        self.bw.level(record)
    }

    pub fn tables(&self, record: Record) -> &'static RecordTables {
        &tables(self.bw)[record.index()]
    }

    pub fn encode(&self, record: Record, frame: &[u8]) -> Vec<u8> {
        encode(self.bw, record, frame)
    }

    pub fn decode(&self, record: Record, llr_tx: &[f64]) -> FrameOut {
        let l = self.level(record);
        let shape = (l.layout.k as usize, l.n(), l.puncture_period() as usize);
        decode_frame(&self.turbo, self.tables(record), shape, whitening(), self.early, llr_tx)
    }
}

pub fn encode(bw: FmBandwidth, record: Record, frame: &[u8]) -> Vec<u8> {
    let l = bw.level(record);
    let t = &tables(bw)[record.index()];
    let mut info = frame_bits(frame, l.layout.k as usize);
    whiten(&mut info, whitening());
    interleave(&turbo_encode(&info, &t.turbo, l.puncture_period(), l.n()), &t.chan)
}
