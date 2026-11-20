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

/// `--tables legacy|2027` and `--vara-dir DIR`: which VARA HF table set to use.
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
                // VARA HF 4.9.3 / FM 4.4.3 (2026-11-20) dropped the 2027-01-01 switch, so there is no `auto` to follow the date.
                if !matches!(s.as_str(), "legacy" | "2027") {
                    return Err(format!("--tables: expected legacy or 2027, not {s}"));
                }
                self.set = Some(s);
            }
            "--vara-dir" => self.vara_dir = Some(v()?.into()),
            // A named keying (xara-keygen) loads like VARA's 2027 files; its xara-keying.txt sets the key offsets.
            "--keying" => (self.set, self.vara_dir) = (Some("2027".into()), Some(v()?.into())),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Apply the selection; call before any modem is built.  `fm` picks VARA FM's tables instead
    /// of HF's.  Returns true when the 2027 set is in use.
    pub fn apply(&self, fm: bool) -> Result<bool, String> {
        if self.set.as_deref() != Some("2027") {
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
