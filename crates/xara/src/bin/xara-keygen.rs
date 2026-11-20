//! `xara-keygen --name NAME --out DIR`: write the named keying NAME to DIR/hf_keying and DIR/fm_keying (see
//! `xara_phy::keying`).  Stations that load the same keying (`--keying DIR/hf_keying`) hear only each other.
use std::path::PathBuf;
use xara_phy::keying;

const USAGE: &str = "usage: xara-keygen --name NAME --out DIR

writes the named keying NAME to DIR/hf_keying and DIR/fm_keying. run every
station of a group with --keying DIR/hf_keying (HF) or DIR/fm_keying (--fm).
the name is the whole definition (same name, same tables, also in XaraNext):
this is not encryption. on amateur bands, publish the name you use.";

fn main() {
    if let Err(e) = run() {
        eprintln!("xara-keygen: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (mut name, mut out) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(k) = args.next() {
        let mut v = || args.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--name" => name = Some(v()?),
            "--out" => out = Some(PathBuf::from(v()?)),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown option {k}\n{USAGE}")),
        }
    }
    let (Some(name), Some(out)) = (name, out) else { return Err(USAGE.into()) };
    keying::check_name(&name)?;
    keying::write(&out.join("hf_keying"), &name, &keying::hf(&name))?;
    keying::write(&out.join("fm_keying"), &name, &keying::fm(&name))?;
    println!("keying {name:?} (generator {}) written to {}", keying::GENERATOR, out.display());
    Ok(())
}
