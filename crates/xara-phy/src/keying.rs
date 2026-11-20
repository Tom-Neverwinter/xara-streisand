//! Named keyings: a group's own HF and FM tables, generated from a name, so its stations hear only each other.
//!
//! Generator version 1 of XaraNext 1.3 (MIT, (c) 2026 x0rro; spec in its `docs/keying.md`), byte for byte: the same
//! name gives the same files here and there, so xara and XaraNext stations on one keying hear each other.  A keying
//! directory has the layout of VARA's 2027 files plus `xara-keying.txt`, and is loaded by the same code
//! (`hf::use_2027_tables`, `fm::code::use_2027_tables`).  Not encryption: the name is the whole definition.

use crate::dat::{fm_tables_with, hf_tables_with, serialize_as};
use crate::fm::level::FmBandwidth;
use crate::hf::level::Bandwidth;
use crate::rnd::{Draw, SplitMix64, fnv1a64};
use crate::seq::{OFDM_BINS, OFDM_SYMBOLS, PSK_CARRIERS, PSK_SYMBOLS};
use std::path::Path;

pub const INFO_FILE: &str = "xara-keying.txt";
/// Bump on any change to the generator, so a published name never changes meaning.
pub const GENERATOR: u32 = 1;

/// What `xara-keying.txt` says: `kind` (`hf` or `fm`) and the control-code offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub fm: bool,
    pub seed_offset: u32,
    /// FM only: the offset on the skip count (VARA FM's 2027 mode: 687, against 933 on the seed).
    pub skip_offset: u32,
}

/// A named keying: its table files and offsets, for one mode.
pub struct Generated {
    pub files: Vec<(&'static str, Vec<u8>)>,
    pub info: Info,
}

pub fn check_name(name: &str) -> Result<(), String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    match name.len() {
        1..=64 if name.chars().all(ok) => Ok(()),
        _ => Err("keying name must be 1-64 characters of A-Z, a-z, 0-9, '-', '_' and '.'".into()),
    }
}

fn stream(name: &str, part: &str) -> SplitMix64 {
    SplitMix64(fnv1a64(format!("xara-keying/{GENERATOR}/{part}/{name}").as_bytes()))
}

/// Keeps clear of 0 (legacy) and of VARA's own 2027 offsets.
fn offset(name: &str, part: &str) -> u32 {
    1024 + stream(name, part).draw(31_000)
}

/// A phase table drawn row by row, written as VARA's `Mod1/2.dat` are: the first `split` symbols of every row, then
/// the rest of every row (the inverse of `seq::psk_layout` / `seq::ofdm_layout`).
fn phase_file(r: &mut SplitMix64, rows: usize, syms: usize, split: usize) -> Vec<u8> {
    let t: Vec<Vec<u8>> = (0..rows).map(|_| (0..syms).map(|_| r.draw(129) as u8).collect()).collect();
    [0..split, split..syms]
        .iter()
        .flat_map(|ks| t.iter().flat_map(move |row| row[ks.clone()].iter().copied()))
        .collect()
}

pub fn hf(name: &str) -> Generated {
    let mut files: Vec<(&'static str, Vec<u8>)> =
        [(Bandwidth::Hz500, "Binary1.dat"), (Bandwidth::Hz2300, "Binary2.dat"), (Bandwidth::Hz2750, "Binary3.dat")]
            .into_iter()
            .map(|(bw, f)| {
                (f, serialize_as(hf_tables_with(bw, &mut stream(name, &format!("tables-{}", bw.hz()))).iter(), true))
            })
            .collect();
    files.push(("Mod1.dat", phase_file(&mut stream(name, "ofdm"), OFDM_BINS, OFDM_SYMBOLS, 409)));
    files.push(("Mod2.dat", phase_file(&mut stream(name, "psk"), PSK_CARRIERS, PSK_SYMBOLS, 411)));
    let seed_offset = offset(name, "seed-offset");
    Generated { files, info: Info { fm: false, seed_offset, skip_offset: seed_offset } }
}

pub fn fm(name: &str) -> Generated {
    // Narrow and wide each start fresh copies of the same streams, so records 0-3 and 16 match across widths.
    let file = |bw| {
        let t = fm_tables_with(bw, &mut stream(name, "fm-tables"), &mut stream(name, "fm-broadcast"));
        serialize_as(t.iter().rev(), false)
    };
    let files = vec![("Binary1.dat", file(FmBandwidth::Narrow)), ("Binary2.dat", file(FmBandwidth::Wide))];
    let info =
        Info { fm: true, seed_offset: offset(name, "fm-seed-offset"), skip_offset: offset(name, "fm-skip-offset") };
    Generated { files, info }
}

/// Write the keying `name` to `dir` (created if needed), never overwriting a file, so a VARA folder can't be damaged.
pub fn write(dir: &Path, name: &str, g: &Generated) -> Result<(), String> {
    check_name(name)?;
    let at = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(dir).map_err(|e| at(dir, e))?;
    let i = g.info;
    let mut info = format!(
        "# xara named keying. Anyone can rebuild these tables with:\n#   xara-keygen --name \"{name}\" --out DIR\n\
         name = {name}\nkind = {}\ngenerator = {GENERATOR}\nseed-offset = {}\n",
        if i.fm { "fm" } else { "hf" },
        i.seed_offset
    );
    if i.fm {
        info += &format!("skip-offset = {}\n", i.skip_offset);
    }
    let files: Vec<_> = g
        .files
        .iter()
        .map(|(n, b)| (dir.join(n), b.as_slice()))
        .chain([(dir.join(INFO_FILE), info.as_bytes())])
        .collect();
    if let Some((p, _)) = files.iter().find(|(p, _)| p.exists()) {
        return Err(format!("{} already exists; choose an empty directory", p.display()));
    }
    files.iter().try_for_each(|(p, b)| std::fs::write(p, b).map_err(|e| at(p, e)))
}

/// `Some` when `dir` holds a named keying, `None` when it is a VARA folder.  Checks the kind against `fm`.
pub fn read_info(dir: &Path, fm: bool) -> Result<Option<Info>, String> {
    let p = dir.join(INFO_FILE);
    if !p.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let field = |key: &str| {
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim())
    };
    let num =
        |key: &str| field(key).and_then(|v| v.parse().ok()).ok_or_else(|| format!("{}: no valid {key}", p.display()));
    let is_fm = field("kind").unwrap_or("hf") == "fm";
    if is_fm != fm {
        let (has, want) = if is_fm { ("FM", "HF") } else { ("HF", "FM") };
        return Err(format!("{} holds an {has} named keying, not an {want} one", dir.display()));
    }
    let seed_offset = num("seed-offset")?;
    let skip_offset = if fm { num("skip-offset")? } else { seed_offset };
    Ok(Some(Info { fm, seed_offset, skip_offset }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XaraNext's pinned values for "NET-7" (its `generator_v1_is_stable` / `fm_generator_v1_is_stable`): matching
    /// them means the same name gives the same keying in both programs.
    #[test]
    fn generator_v1_matches_xaranext() {
        let h = hf("NET-7");
        let sum =
            h.files.iter().map(|(_, b)| fnv1a64(b)).fold(u64::from(h.info.seed_offset), |a, x| a.rotate_left(7) ^ x);
        assert_eq!(sum, 0xb3ab_2141_17f1_9826, "HF {sum:#x}");
        let f = fm("NET-7");
        let (s, k) = (u64::from(f.info.seed_offset), u64::from(f.info.skip_offset));
        let sum = f.files.iter().map(|(_, b)| fnv1a64(b)).fold(s << 16 | k, |a, x| a.rotate_left(7) ^ x);
        assert_eq!(sum, 0x724b_885f_015c_9aeb, "FM {sum:#x}");
    }

    #[test]
    fn names_and_files() {
        assert!(check_name("").is_err() && check_name("two words").is_err() && check_name("Net_7.b-2").is_ok());
        let dir = std::env::temp_dir().join(format!("xara-keying-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_info(&dir, false), Ok(None));
        let g = hf("NET-7");
        write(&dir, "NET-7", &g).unwrap();
        assert_eq!(read_info(&dir, false), Ok(Some(g.info)));
        assert!(read_info(&dir, true).unwrap_err().contains("HF named keying"));
        assert!(write(&dir, "NET-7", &g).unwrap_err().contains("already exists"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
