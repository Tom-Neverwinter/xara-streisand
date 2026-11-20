//! A named FM keying loads like VARA FM's 2027 files, keeps narrow and wide able to connect (records 0-3 and 16 shared,
//! unlike VARA's 2027 tables), and offsets the codebook seed and skip count separately.
use xara_phy::dat::fm_tables;
use xara_phy::fm::level::FmBandwidth;
use xara_phy::keying;

#[test]
fn named_fm_keying_loads() {
    let dir = std::env::temp_dir().join(format!("xara-fm-named-keying-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let g = keying::fm("NET-7");
    keying::write(&dir, "NET-7", &g).unwrap();
    assert!(xara_phy::hf::use_2027_tables(&dir).unwrap_err().contains("FM named keying"));
    xara_phy::fm::code::use_2027_tables(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(xara_phy::fm::code::key_shifts(), (g.info.seed_offset, g.info.skip_offset));
    let (n, w) = (xara_phy::fm::code::tables(FmBandwidth::Narrow), xara_phy::fm::code::tables(FmBandwidth::Wide));
    for r in [0, 1, 2, 3, 16] {
        assert_eq!(n[r], w[r], "record {r} shared by both widths");
    }
    assert_ne!(n[0].chan, fm_tables(FmBandwidth::Narrow)[0].chan, "not the legacy keying");
}
