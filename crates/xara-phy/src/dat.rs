use crate::crc::crc16_genibus;
use crate::fm::level::{FmBandwidth, Layout, Record};
use crate::hf::level::{Bandwidth, FSK_PILOTS, Level, Waveform};
use crate::rnd::VbRnd;

const HF_PP: u32 = crate::hf::ofdm::PILOT_PERIOD;
const FM_PP: u32 = crate::fm::level::PILOT_PERIOD;

pub const TABLE_SEED: f64 = 43.0;
pub const FM_RECORD16_SEED: f64 = 77.0;
/// VARA SAT's interleaver and turbo tables come from the same generator as HF's, seeded with 99 (checked by decoding
/// recorded VARA SAT frames, `tests/sat_frames.rs`).
pub const SAT_TABLE_SEED: f64 = 99.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordTables {
    pub chan: Vec<u32>,
    pub turbo: Vec<u32>,
    pub other: Vec<u16>,
}

pub(crate) fn marker_map(other: &[u16], nc: usize, markers: usize, nsym: usize) -> Vec<Vec<bool>> {
    other[nc * markers..].chunks(nsym).take(nc).map(|r| r.iter().map(|&v| v != 0).collect()).collect()
}

fn perm(r: &mut VbRnd, n: u32) -> Vec<u32> {
    let mut used = vec![false; n as usize];
    let mut out = Vec::with_capacity(n as usize);
    while out.len() < n as usize {
        let v = r.draw(n);
        if !std::mem::replace(&mut used[v as usize], true) {
            out.push(v);
        }
    }
    out
}

fn fsk_other(r: &mut VbRnd, l: &Level) -> Vec<u16> {
    let Waveform::Fsk { symbols: s, first, last, step, .. } = l.waveform else { unreachable!() };
    let nb = last - first + 1;
    let m = if step > 1 { nb / step + 1 } else { nb };
    let mut out: Vec<u16> =
        (0..s - 1).map(|i| (first + u32::from(step > 1 && i % 2 == 0) + step * r.draw(m)) as u16).collect();
    let inc = (s - 2) as f32 / FSK_PILOTS as f32;
    let mut acc = 1.0f32;
    let rows: Vec<u32> = (0..FSK_PILOTS)
        .map(|_| {
            acc += inc;
            acc as u32
        })
        .collect();
    out.extend(rows.iter().map(|&x| x as u16));
    let scale = (1u32 << l.bps) / 8;
    let mut pilot = vec![None; (s - 1) as usize];
    for &row in &rows {
        pilot[(row - 2) as usize] = Some(scale * r.draw(8));
    }
    for p in pilot {
        out.extend(match p {
            Some(v) => [1, v as u16],
            None => [0, 0],
        });
    }
    out
}

fn marker_rows(nc: u32, markers: u32, mut next: impl FnMut(u32) -> u32, gap: u32) -> Vec<Vec<u32>> {
    (0..nc)
        .map(|c| {
            let mut row: Vec<u32> = Vec::with_capacity(markers as usize);
            while row.len() < markers as usize {
                let v = next(c);
                if row.iter().all(|&w| v.abs_diff(w) > gap) {
                    row.push(v);
                }
            }
            row
        })
        .collect()
}

fn carrier_other(rows: Vec<Vec<u32>>, spf: u32) -> Vec<u16> {
    let mut out: Vec<u16> = rows.iter().flatten().map(|&v| v as u16).collect();
    for row in &rows {
        let mut m = vec![0u16; (spf - 1) as usize];
        for &v in row {
            m[(v - 2) as usize] = 1;
        }
        out.extend(m);
    }
    out
}

fn psk_other(r: &mut VbRnd, l: &Level) -> Vec<u16> {
    let (spf, first) = match l.waveform {
        Waveform::Ofdm { spf, first, .. } => (spf, first),
        _ => (l.spf(), 0),
    };
    let qam = l.is_qam();
    let rows = marker_rows(
        l.nc(),
        l.markers(),
        |c| if qam { 3 + (first + c) % 2 + HF_PP * r.draw((spf - 3) / HF_PP) } else { 5 + r.draw(spf - 8) },
        0,
    );
    carrier_other(rows, spf)
}

fn fm_other(r: &mut VbRnd, f: &Layout) -> Vec<u16> {
    let gap = if f.spf < 50 { 0 } else { 2 };
    let rows = marker_rows(
        f.nc,
        f.markers,
        |c| {
            if f.bps > 2 {
                3 + (f.first + f.step * c) % 2 + FM_PP * r.draw((f.spf - 3) / FM_PP)
            } else {
                3 + r.draw(f.spf - 2)
            }
        },
        gap,
    );
    carrier_other(rows, f.spf)
}

pub fn hf_tables(bw: Bandwidth) -> Vec<RecordTables> {
    level_tables(bw.table(), if bw == Bandwidth::Sat { SAT_TABLE_SEED } else { TABLE_SEED })
}

fn level_tables(levels: &[Level], seed: f64) -> Vec<RecordTables> {
    let mut r = VbRnd::stream(seed);
    levels
        .iter()
        .map(|l| {
            let chan = perm(&mut r, l.n());
            let turbo = perm(&mut r, l.k);
            let other = if l.is_fsk() { fsk_other(&mut r, l) } else { psk_other(&mut r, l) };
            debug_assert_eq!(other.len() as u32, l.other_len());
            RecordTables { chan, turbo, other }
        })
        .collect()
}

pub fn fm_tables(bw: FmBandwidth) -> Vec<RecordTables> {
    let mut r = VbRnd::stream(TABLE_SEED);
    bw.layouts()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i == Record::Broadcast.index() {
                r = VbRnd::stream(FM_RECORD16_SEED);
            }
            let chan = perm(&mut r, f.n());
            let turbo = perm(&mut r, f.k);
            let other = fm_other(&mut r, f);
            RecordTables { chan, turbo, other }
        })
        .collect()
}

fn put_perm(out: &mut Vec<u8>, p: &[u32]) {
    if p.len() > 65536 {
        out.extend(p.iter().flat_map(|v| v.to_be_bytes()[1..].to_vec()));
    } else {
        out.extend(p.iter().flat_map(|&v| (v as u16).to_be_bytes()));
    }
}

fn serialize(tables: &[RecordTables]) -> Vec<u8> {
    let mut out = Vec::new();
    for t in tables {
        put_perm(&mut out, &t.chan);
        put_perm(&mut out, &t.turbo);
        out.extend(t.other.iter().flat_map(|v| v.to_be_bytes()));
    }
    let crc = crc16_genibus(&out[..out.len() - 1]);
    out.extend(crc.to_be_bytes());
    out
}

/// Parse a VARA HF table file (`Binary1..6.dat`), the inverse of `serialize`.  The 2027 files
/// (`Binary1-3`) store each record's turbo permutation before its channel permutation
/// (`turbo_first`); the legacy files (`Binary4-6`) store the channel permutation first.
pub fn parse_hf_dat(bw: Bandwidth, raw: &[u8], turbo_first: bool) -> Result<Vec<RecordTables>, String> {
    if raw.len() < 3 || crc16_genibus(&raw[..raw.len() - 3]).to_be_bytes() != raw[raw.len() - 2..] {
        return Err("CRC trailer mismatch: not an original VARA table file".into());
    }
    let mut pos = 0usize;
    let mut take = |n: usize, w: usize| -> Result<Vec<u32>, String> {
        let b = raw.get(pos..pos + n * w).filter(|_| pos + n * w <= raw.len() - 2).ok_or("table file too short")?;
        pos += n * w;
        Ok(b.chunks(w).map(|c| c.iter().fold(0u32, |a, &x| a << 8 | u32::from(x))).collect())
    };
    let width = |n: u32| if n > 65536 { 3 } else { 2 };
    let is_perm = |p: &[u32]| {
        let mut seen = vec![false; p.len()];
        p.iter().all(|&v| (v as usize) < p.len() && !std::mem::replace(&mut seen[v as usize], true))
    };
    let mut out = Vec::new();
    for l in bw.table() {
        let (a, b) = if turbo_first { (l.k, l.n()) } else { (l.n(), l.k) };
        let first = take(a as usize, width(a) as usize)?;
        let second = take(b as usize, width(b) as usize)?;
        let (chan, turbo) = if turbo_first { (second, first) } else { (first, second) };
        let other = take(l.other_len() as usize, 2)?.into_iter().map(|v| v as u16).collect();
        if !is_perm(&chan) || !is_perm(&turbo) {
            return Err(format!("record for level {} is not a permutation", out.len() + 1));
        }
        out.push(RecordTables { chan, turbo, other });
    }
    if pos != raw.len() - 2 {
        return Err(format!("{} unexpected bytes after the last record", raw.len() - 2 - pos));
    }
    Ok(out)
}

/// Parse a VARA FM table file.  Legacy `Binary3/4.dat` store records 0..=16 in order; VARA FM
/// 4.4.1's 2027 files `Binary1/2.dat` store them in reverse (`reversed`).  Channel permutation
/// first in both.
pub fn parse_fm_dat(bw: FmBandwidth, raw: &[u8], reversed: bool) -> Result<Vec<RecordTables>, String> {
    if raw.len() < 3 || crc16_genibus(&raw[..raw.len() - 3]).to_be_bytes() != raw[raw.len() - 2..] {
        return Err("CRC trailer mismatch: not an original VARA table file".into());
    }
    let mut pos = 0usize;
    let mut take = |n: u32| -> Result<Vec<u32>, String> {
        let w = if n > 65536 { 3 } else { 2 };
        let end = pos + n as usize * w;
        let b = raw.get(pos..end).filter(|_| end <= raw.len() - 2).ok_or("table file too short")?;
        pos = end;
        Ok(b.chunks(w).map(|c| c.iter().fold(0u32, |a, &x| a << 8 | u32::from(x))).collect())
    };
    let is_perm = |p: &[u32]| {
        let mut seen = vec![false; p.len()];
        p.iter().all(|&v| (v as usize) < p.len() && !std::mem::replace(&mut seen[v as usize], true))
    };
    let layouts = bw.layouts();
    let order: Vec<usize> = if reversed { (0..layouts.len()).rev().collect() } else { (0..layouts.len()).collect() };
    let mut out = vec![None; layouts.len()];
    for i in order {
        let f = &layouts[i];
        let chan = take(f.n())?;
        let turbo = take(f.k)?;
        let other = take(f.other_len())?.into_iter().map(|v| v as u16).collect();
        if !is_perm(&chan) || !is_perm(&turbo) {
            return Err(format!("record {i} is not a permutation"));
        }
        out[i] = Some(RecordTables { chan, turbo, other });
    }
    if pos != raw.len() - 2 {
        return Err(format!("{} unexpected bytes after the last record", raw.len() - 2 - pos));
    }
    Ok(out.into_iter().map(Option::unwrap).collect())
}

pub fn hf_dat(bw: Bandwidth) -> Vec<u8> {
    serialize(&hf_tables(bw))
}

pub fn fm_dat(bw: FmBandwidth) -> Vec<u8> {
    serialize(&fm_tables(bw))
}
