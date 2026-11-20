use crate::rnd::VbRnd;

pub const HF_SEED: f64 = 292.0;
pub const FM_SEED: f64 = 43.0;
/// VARA SAT data-frame whitening seed (found from lab audio: the whitening that makes recorded SAT frames pass CRC).
pub const SAT_SEED: f64 = 414.0;
pub const WHITENING_LEN: usize = 150_000;
pub const PSK_CARRIERS: usize = 13;
pub const PSK_SYMBOLS: usize = 471;
pub const OFDM_BINS: usize = 128;
pub const OFDM_SYMBOLS: usize = 469;
pub const DATA_DAT_LEN: usize = 65_000;

pub fn whitening(seed: f64) -> Vec<u8> {
    let mut r = VbRnd::stream(seed);
    (0..WHITENING_LEN).map(|_| r.rand_int(1) as u8).collect()
}

/// PSK training phasors; `seed` is the bandwidth's whitening seed (HF_SEED, or SAT_SEED for VARA SAT).
pub fn psk_phasor_index(seed: f64) -> Vec<[u8; PSK_SYMBOLS]> {
    let mut r = VbRnd::stream(seed);
    psk_layout(std::iter::from_fn(|| Some(r.rand_int(128) as u8)))
}

/// Narrowband-PSK phasor indices laid out carrier-major, symbols 0..411 then 411..471:
/// from a seeded stream (legacy, SAT) or from VARA 4.9.1's `Mod2.dat` (2027 set).
pub fn psk_layout(mut it: impl Iterator<Item = u8>) -> Vec<[u8; PSK_SYMBOLS]> {
    let mut d = vec![[0u8; PSK_SYMBOLS]; PSK_CARRIERS];
    for ks in [0..411, 411..PSK_SYMBOLS] {
        for row in d.iter_mut() {
            for k in ks.clone() {
                row[k] = it.next().expect("PSK phasor source too short");
            }
        }
    }
    d
}

pub fn data_dat(seed: f64) -> Vec<u8> {
    let mut r = VbRnd::stream(seed);
    (0..DATA_DAT_LEN).map(|_| r.rand_int(128) as u8).collect()
}

/// OFDM training phasors from the bandwidth's whitening seed, like `psk_phasor_index`.
pub fn ofdm_phasor_index(seed: f64) -> Vec<[u8; OFDM_SYMBOLS]> {
    ofdm_layout(data_dat(seed).into_iter())
}

/// OFDM phasor indices, 128 bins, symbols 0..409 then 409..469: from a seeded `Data.dat` (legacy, SAT)
/// or from VARA 4.9.1's `Mod1.dat` (2027 set).
pub fn ofdm_layout(mut it: impl Iterator<Item = u8>) -> Vec<[u8; OFDM_SYMBOLS]> {
    let mut out = vec![[0u8; OFDM_SYMBOLS]; OFDM_BINS];
    for ms in [0..409, 409..OFDM_SYMBOLS] {
        for row in out.iter_mut() {
            for m in ms.clone() {
                row[m] = it.next().expect("OFDM phasor source too short");
            }
        }
    }
    out
}
