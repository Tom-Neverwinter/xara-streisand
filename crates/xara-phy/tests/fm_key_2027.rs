//! VARA FM 4.4.1's 2027 mode seeds its control codebooks with both call keys + 933 (lab: two real VARA FM stations
//! with their clock in 2027). Needs a VARA FM 4.4.1 folder in XARA_VARA_FM_DIR (skipped without one).
use xara_phy::call::call_hash;
use xara_phy::fm::ctrl::{Codebook, KEY_SHIFT_2027};

#[test]
fn the_2027_codebook_is_the_2026_one_at_the_shifted_keys() {
    let Some(dir) = std::env::var_os("XARA_VARA_FM_DIR") else {
        eprintln!("XARA_VARA_FM_DIR not set: skipped");
        return;
    };
    let (d, u) = (call_hash(b"N1CALL"), call_hash(b"N0CALL"));
    let want = Codebook::new(d + KEY_SHIFT_2027, u + KEY_SHIFT_2027);
    let legacy = Codebook::new(d, u);
    xara_phy::fm::code::use_2027_tables(std::path::Path::new(&dir)).unwrap();
    let got = Codebook::new(d, u);
    for c in 1..=24 {
        assert_eq!(got.code(c), want.code(c), "code {c}");
    }
    assert_ne!(got.code(24), legacy.code(24), "the connect reply differs from 2026's");
}
