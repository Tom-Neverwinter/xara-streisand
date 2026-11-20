use std::fmt::Display;
use std::process::ExitCode;
use std::str::FromStr;

pub fn num<T: FromStr>(k: &str, v: &str) -> Result<T, String>
where
    T::Err: Display,
{
    v.parse().map_err(|e| format!("{k}: {e}"))
}

pub fn either(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

pub fn call(c: &str) -> Result<String, String> {
    xara_host::command::valid_call(c).ok_or_else(|| format!("bad call {c}"))
}

pub fn is_meta(k: &str) -> bool {
    matches!(k, "--help" | "-h" | "--version" | "-V")
}

// The root README, not crates/xara/README.md: that is a git symlink, which Windows checks out as a text file.
const README: &str = include_str!("../../../README.md");
const FOOTER: &str = "copyright (c) 2026 x0rro\nreleased under MIT license, see LICENSE\n";

pub struct Usage {
    pub name: &'static str,
    pub synopsis: &'static str,
    pub tagline: &'static str,
    pub sections: &'static [&'static str],
}

impl Usage {
    pub fn help(&self) -> String {
        let (mut out, mut keep, mut gap) = (format!("{}\n{}\n", self.synopsis, self.tagline), false, true);
        for l in README.lines() {
            if let Some(h) = l.strip_prefix("## ") {
                keep = self.sections.contains(&h.trim());
                gap = true;
            } else if keep && !l.trim().is_empty() && l.starts_with("    ") {
                if gap {
                    out.push('\n');
                }
                out.push_str(l.trim_end());
                out.push('\n');
                gap = false;
            } else {
                gap = true;
            }
        }
        out.push('\n');
        out.push_str(FOOTER);
        out
    }
}

pub fn meta(k: &str, usage: &Usage) -> bool {
    match k {
        "--help" | "-h" => print!("{}", usage.help()),
        "--version" | "-V" => println!("{} {}", usage.name, env!("CARGO_PKG_VERSION")),
        _ => return false,
    }
    true
}

pub fn args<A>(r: Result<Option<A>, String>, usage: &Usage) -> Result<A, ExitCode> {
    match r {
        Ok(Some(a)) => Ok(a),
        Ok(None) => Err(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("{e}\n{}\nsee {} --help", usage.synopsis, usage.name);
            Err(ExitCode::from(2))
        }
    }
}

/// `--tables legacy|2027|auto` and `--vara-dir DIR`: which VARA HF table set to use.
#[derive(Default)]
pub struct TableArgs {
    pub set: Option<String>,
    pub vara_dir: Option<std::path::PathBuf>,
}

impl TableArgs {
    pub fn take(&mut self, k: &str, v: impl FnOnce() -> Result<String, String>) -> Result<bool, String> {
        match k {
            "--tables" => {
                let s = v()?.to_ascii_lowercase();
                if !matches!(s.as_str(), "legacy" | "2027" | "auto") {
                    return Err(format!("--tables: expected legacy, 2027 or auto, not {s}"));
                }
                self.set = Some(s);
            }
            "--vara-dir" => self.vara_dir = Some(v()?.into()),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Apply the selection; call before any modem is built.  `fm` picks VARA FM's tables instead
    /// of HF's.  Returns true when the 2027 set is in use.
    pub fn apply(&self, fm: bool) -> Result<bool, String> {
        let new = match self.set.as_deref().unwrap_or("legacy") {
            "2027" => true,
            "auto" => utc_year(std::time::SystemTime::now()) >= 2027,
            _ => false,
        };
        if !new {
            return Ok(false);
        }
        let dir =
            self.vara_dir.as_ref().ok_or("--tables 2027 needs --vara-dir (a VARA HF 4.9.1 or FM 4.4.1 folder)")?;
        match fm {
            true => xara_phy::fm::code::use_2027_tables(dir),
            false => xara_phy::hf::use_2027_tables(dir),
        }
        .map(|()| true)
    }
}

// shortcut: VARA switches on local time and this uses UTC, so `auto` can be up to a day off at New Year; use --tables 2027 to force it.
fn utc_year(t: std::time::SystemTime) -> i64 {
    let days = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86400) as i64;
    // civil-from-days (Howard Hinnant), proleptic Gregorian
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let m = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(m >= 10)
}

#[cfg(test)]
mod table_tests {
    use super::utc_year;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn year_boundaries() {
        let at = |s: u64| utc_year(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), 1970);
        assert_eq!(at(1_798_761_599), 2026); // 2026-12-31 23:59:59 UTC
        assert_eq!(at(1_798_761_600), 2027); // 2027-01-01 00:00:00 UTC
        assert_eq!(at(951_782_400), 2000); // 2000-02-29
    }
}

#[derive(Default)]
pub struct UiArgs {
    pub port: Option<u16>,
    pub addr: Option<String>,
    pub hosts: Vec<String>,
}

impl UiArgs {
    pub fn take(&mut self, k: &str, v: impl FnOnce() -> Result<String, String>) -> Result<bool, String> {
        match k {
            "--ui-port" => {
                let p = num(k, &v()?)?;
                if p == 0 {
                    return Err("--ui-port: must be 1-65535".into());
                }
                self.port = Some(p);
            }
            "--ui-addr" => self.addr = Some(v()?),
            "--ui-host" => self.hosts.push(v()?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub fn addr(&self) -> &str {
        self.addr.as_deref().unwrap_or(crate::DEFAULT_ADDR)
    }
}
