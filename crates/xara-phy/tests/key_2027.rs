//! VARA HF 4.9.1's 2027 mode seeds its control codebooks with every call key + 687 (lab: real VARA HF, clock in
//! 2027). Needs a VARA HF 4.9.1 folder in XARA_VARA_DIR (skipped without one): its own process, as the switch is.
use xara_phy::call::call_hash;
use xara_phy::hf::ctrl::{Codebook, Fmt, KEY_SHIFT_2027, Role};
use xara_phy::hf::level::Bandwidth;

#[test]
fn the_2027_codebook_is_the_2026_one_at_the_shifted_key() {
    let Ok(dir) = std::env::var("XARA_VARA_DIR") else {
        eprintln!("XARA_VARA_DIR not set: skipped");
        return;
    };
    let (n0, n1) = (call_hash(b"N0CALL"), call_hash(b"N1CALL"));
    let book = |seed: u32, me: u32, role| {
        let mut b = Codebook::new(seed, [me, 0, 0, 0, 0], role, Bandwidth::Hz2300);
        b.set_connect_codes(n0, role);
        b
    };
    let s = KEY_SHIFT_2027;
    // what the 2027 book must be: the legacy generator at keys + 687 (the empty alias slots shift too)
    let want = {
        let mut b = Codebook::new(n1 + s, [n1 + s, s, s, s, s], Role::Answerer, Bandwidth::Hz2300);
        b.set_connect_codes(n0 + s, Role::Answerer);
        b
    };
    let legacy = book(n1, n1, Role::Answerer);
    xara_phy::hf::use_2027_tables(std::path::Path::new(&dir)).unwrap();
    let got = book(n1, n1, Role::Answerer);
    for f in Fmt::ALL {
        for c in 1..=50 {
            assert_eq!(got.rx(f, c), want.rx(f, c), "rx {c} f{f}");
            assert_eq!(got.tx(f, c), want.tx(f, c), "tx {c} f{f}");
        }
    }
    // the connect request VARA 2027 sends for N1CALL differs from 2026's
    assert_ne!(got.rx(Fmt::F0, 26), legacy.rx(Fmt::F0, 26));
}
