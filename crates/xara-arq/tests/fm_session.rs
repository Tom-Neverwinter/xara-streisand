use xara_arq::compression::Compression;
use xara_arq::fm;
use xara_arq::host::HostEvent;
use xara_arq::textcomp::compress;
use xara_arq::time::{SECOND, Time};
use xara_phy::fm::ctrl::code::{ACK_STAY, CONNECT_REPLY_NARROW, GO_AHEAD, NACK};
use xara_phy::fm::frame::{Trailer, build_data};
use xara_phy::fm::level::{FmBandwidth, Record};

fn drain(s: &mut fm::Session) -> Vec<fm::Output> {
    std::iter::from_fn(|| s.pop()).collect()
}

fn ctrl_codes(out: &[fm::Output]) -> Vec<u32> {
    out.iter()
        .filter_map(|o| if let fm::Output::Tx(fm::Tx::Ctrl { code, .. }) = o { Some(*code) } else { None })
        .collect()
}

fn delivered(out: &[fm::Output]) -> Vec<u8> {
    out.iter()
        .filter_map(|o| if let fm::Output::Host(HostEvent::Data(d)) = o { Some(d.clone()) } else { None })
        .flatten()
        .collect()
}

fn text_receiver() -> (fm::Session, Time) {
    let mut cfg = fm::Config::new(FmBandwidth::Narrow, "N0CALL");
    (cfg.host.registered, cfg.host.compression) = (true, Compression::Text);
    let mut s = fm::Session::new(cfg);
    assert!(s.connect(0, None, "N1CALL", &[]));
    drain(&mut s);
    s.on_tx_done(SECOND);
    let t = SECOND + SECOND / 2;
    s.on_ctrl(t, fm::CtrlIn { code: CONNECT_REPLY_NARROW, lead_row: 1, t_end: t });
    assert_eq!(ctrl_codes(&drain(&mut s)), [GO_AHEAD]);
    assert_eq!(s.state(), fm::State::Receiving);
    s.on_tx_done(2 * SECOND);
    (s, 3 * SECOND)
}

fn start_frame(data: &[u8]) -> fm::DataIn {
    let len = FmBandwidth::Narrow.level(Record::Start).frame_bytes();
    let trailer = Trailer { lead_report: 1, level: 0, chain_snr: None };
    let bytes = build_data(Record::Start, len, data, true, 2, false, trailer);
    fm::DataIn { record: Record::Start, frames: vec![fm::FrameIn { ok: true, bytes, snr_db: 30.0 }], t_end: 3 * SECOND }
}

#[test]
fn compressed_frame_is_expanded_and_acked() {
    let (mut s, now) = text_receiver();
    let e = compress(b"the message ", 10, 1);
    s.on_data(now, start_frame(&e.bytes));
    let out = drain(&mut s);
    assert_eq!(delivered(&out), &b"the message "[..e.consumed]);
    assert_eq!(ctrl_codes(&out), [ACK_STAY]);
}

#[test]
fn compressed_frame_that_does_not_expand_is_nacked() {
    let (mut s, now) = text_receiver();
    s.on_data(now, start_frame(&[0]));
    let out = drain(&mut s);
    assert!(delivered(&out).is_empty(), "{out:?}");
    assert_eq!(ctrl_codes(&out), [NACK]);
}

#[test]
fn compressed_frame_with_a_lost_frame_2_is_still_expanded() {
    let (mut s, now) = text_receiver();
    let e = compress(b"[WL2K-5.0-B2FWIHJM$]\r", 30, 1);
    let mut d = start_frame(&e.bytes);
    d.frames.push(fm::FrameIn { ok: false, bytes: Vec::new(), snr_db: 0.0 });
    s.on_data(now, d);
    let out = drain(&mut s);
    assert_eq!(delivered(&out), &b"[WL2K-5.0-B2FWIHJM$]\r"[..e.consumed], "{out:?}");
}

/// A listening station answers VARA FM's Ping with a Reply carrying the S/N it heard, then CANCELPENDING.
#[test]
fn a_direct_ping_gets_a_reply() {
    use xara_phy::fm::frame::{Kind, Record0, snr_byte};
    let mut s = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N1CALL"));
    drain(&mut s);
    let ping = hex_ping();
    s.on_record0(0, &ping, 20.0);
    let out = drain(&mut s);
    assert!(matches!(out[0], fm::Output::Host(HostEvent::Pending)), "{out:?}");
    let reply = out
        .iter()
        .find_map(|o| if let fm::Output::Tx(fm::Tx::Data { frames, .. }) = o { Some(frames[0].clone()) } else { None })
        .expect("a reply on the air");
    let r = Record0::parse(&reply).unwrap();
    assert_eq!((r.kind, r.source.as_str(), r.target.as_str(), r.wide), (Kind::Reply, "N1CALL", "N0CALL", true));
    assert_eq!(r.snr, snr_byte(20.0 + fm::adapt::SNR_OFFSET_DB));
    s.on_tx_done(SECOND);
    assert!(drain(&mut s).iter().any(|o| matches!(o, fm::Output::Host(HostEvent::CancelPending))));
    assert_eq!(s.state(), fm::State::Listen);
    // Not ours, or through a digipeater: no answer.
    for other in [
        Record0 { target: "N9XYZ".into(), ..Record0::parse(&ping).unwrap() },
        Record0 { via1: "N2CALL".into(), ..Record0::parse(&ping).unwrap() },
    ] {
        s.on_record0(2 * SECOND, &other.to_bytes().unwrap(), 20.0);
        assert!(drain(&mut s).is_empty());
    }
}

/// The Ping real VARA FM 4.4.1 sent from N0CALL to N1CALL (wide) in the lab, 2026-11-18.
fn hex_ping() -> Vec<u8> {
    let h = "424e3043414c4c2000202020202020200020202020202020004e3143414c4c200000000100f67d";
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect()
}
