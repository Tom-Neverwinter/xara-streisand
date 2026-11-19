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
    // Not ours, or not yet through its digipeater: no answer.
    for other in [
        Record0 { target: "N9XYZ".into(), ..Record0::parse(&ping).unwrap() },
        Record0 { via1: "N2CALL".into(), ..Record0::parse(&ping).unwrap() },
    ] {
        s.on_record0(2 * SECOND, &other.to_bytes().unwrap(), 20.0);
        assert!(drain(&mut s).is_empty());
    }
}

/// A Ping that came through digipeater N9DIG is answered back along the path: the exact Reply real VARA FM 4.4.1
/// sent in the lab (N0CALL -> N9DIG -> N1CALL, wide, 2026-11-18).
#[test]
fn a_digipeated_ping_gets_a_reply_back_through_the_digi() {
    let mut s = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N1CALL"));
    drain(&mut s);
    s.on_record0(0, &unhex("4a4e3043414c4c20004e3944494720200020202020202020004e3143414c4c2000000001f53c8c"), 40.0);
    let reply = drain(&mut s)
        .into_iter()
        .find_map(|o| if let fm::Output::Tx(fm::Tx::Data { frames, .. }) = o { Some(frames[0].clone()) } else { None })
        .expect("a reply on the air");
    assert_eq!(reply, unhex("624e3143414c4c20004e3944494720200020202020202020004e3043414c4c2000f5000100b762"));
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect()
}

/// The Ping real VARA FM 4.4.1 sent from N0CALL to N1CALL (wide) in the lab, 2026-11-18.
fn hex_ping() -> Vec<u8> {
    unhex("424e3043414c4c2000202020202020200020202020202020004e3143414c4c200000000100f67d")
}

/// VARA FM's Autotune: stay quiet through the ten louder-and-louder records, then name the step three above the
/// first one heard (byte 34 = step << 2).
#[test]
fn autotune_replies_three_steps_above_the_first_heard() {
    use xara_phy::fm::frame::{Kind, Record0};
    let mut s = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N1CALL"));
    drain(&mut s);
    let step = |n: u8| {
        let r = Record0 {
            kind: Kind::AutoTune,
            snr: n,
            control: 1,
            ..Record0::connect("N0CALL", "N1CALL", "", "", false, true)
        };
        r.to_bytes().unwrap()
    };
    let mut out = Vec::new();
    for n in 3..=10 {
        s.on_record0(i64::from(n) * SECOND, &step(n), 20.0);
        out.extend(drain(&mut s));
    }
    let pending = out.iter().filter(|o| matches!(o, fm::Output::Host(HostEvent::Pending))).count();
    let replies: Vec<Vec<u8>> = out
        .iter()
        .filter_map(
            |o| if let fm::Output::Tx(fm::Tx::Data { frames, .. }) = o { Some(frames[0].clone()) } else { None },
        )
        .collect();
    assert_eq!((pending, replies.len()), (1, 1), "{out:?}");
    let r = Record0::parse(&replies[0]).unwrap();
    assert_eq!((r.kind, r.tune, r.target.as_str()), (Kind::Reply, 6 << 2, "N0CALL"));
    s.on_tx_done(11 * SECOND);
    assert!(drain(&mut s).iter().any(|o| matches!(o, fm::Output::Host(HostEvent::CancelPending))));
}

/// Step 10 lost: the Autotune reply still goes out, once step 10 was due and a grace period has passed.
#[test]
fn autotune_replies_when_the_last_step_is_lost() {
    use xara_phy::fm::frame::{Kind, Record0};
    let mut s = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N1CALL"));
    drain(&mut s);
    let step = |n: u8| {
        Record0 {
            kind: Kind::AutoTune,
            snr: n,
            control: 1,
            ..Record0::connect("N0CALL", "N1CALL", "", "", false, true)
        }
        .to_bytes()
        .unwrap()
    };
    let replies = |s: &mut fm::Session| {
        drain(s)
            .into_iter()
            .filter_map(|o| {
                if let fm::Output::Tx(fm::Tx::Data { frames, .. }) = o { Some(frames[0].clone()) } else { None }
            })
            .collect::<Vec<_>>()
    };
    let period = SECOND * 136 / 100;
    for n in 1..=8u8 {
        s.on_record0(i64::from(n) * period, &step(n), 20.0);
    }
    // Step 10 is due at 10 periods; still waiting just after it.
    assert_eq!(s.next_deadline(), Some(10 * period + SECOND / 2));
    s.poll(10 * period + SECOND / 4);
    assert!(replies(&mut s).is_empty());
    s.poll(10 * period + SECOND);
    let out = replies(&mut s);
    assert_eq!(out.len(), 1);
    let r = Record0::parse(&out[0]).unwrap();
    assert_eq!((r.kind, r.tune, r.target.as_str()), (Kind::Reply, 4 << 2, "N0CALL"));
}

/// The record a session put on the air: (Autotune level if it was a tune burst, the record bytes).
fn sent(out: &[fm::Output]) -> Vec<(Option<f64>, Vec<u8>)> {
    out.iter()
        .filter_map(|o| match o {
            fm::Output::Tx(fm::Tx::Data { frames, .. }) => Some((None, frames[0].clone())),
            fm::Output::Tx(fm::Tx::Tune { frame, level_db }) => Some((Some(*level_db), frame.clone())),
            _ => None,
        })
        .collect()
}

fn probes(out: &[fm::Output]) -> Vec<(String, Option<f64>, Option<u8>)> {
    out.iter()
        .filter_map(|o| match o {
            fm::Output::Host(HostEvent::Probe { call, sn, step }) => Some((call.clone(), *sn, *step)),
            _ => None,
        })
        .collect()
}

/// xara pings: one Ping record to the target, then the target's reply reports the S/N it heard us at.
#[test]
fn ping_reports_the_answer() {
    use xara_phy::fm::frame::{Kind, Record0};
    let mut a = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N0CALL"));
    drain(&mut a);
    assert!(a.probe(0, "N0CALL", "N1CALL", &[], false));
    let tx = sent(&drain(&mut a));
    let r = Record0::parse(&tx[0].1).unwrap();
    assert_eq!((tx.len(), r.kind, r.source.as_str(), r.target.as_str()), (1, Kind::Ping, "N0CALL", "N1CALL"));
    assert!(!a.probe(0, "N0CALL", "N1CALL", &[], false), "one probe at a time");
    a.on_tx_done(SECOND);
    let reply = Record0::ping_reply("N1CALL", "N0CALL", true, 140).to_bytes().unwrap();
    a.on_record0(2 * SECOND, &reply, 20.0);
    assert_eq!(probes(&drain(&mut a)), [("N1CALL".to_string(), Some(20.0), None)]);
}

/// Autotune with nobody answering: ten bursts 1.36 s apart, quietest first, then "no answer".
#[test]
fn autotune_sends_ten_rising_bursts_then_gives_up() {
    use xara_phy::fm::frame::{Kind, Record0};
    let mut a = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N0CALL"));
    drain(&mut a);
    assert!(a.probe(0, "N0CALL", "N1CALL", &[], true));
    let mut bursts = Vec::new();
    let mut now: Time = 0;
    for _ in 0..20 {
        let out = drain(&mut a);
        bursts.extend(sent(&out));
        if !probes(&out).is_empty() {
            assert_eq!(probes(&out), [("N1CALL".to_string(), None, Some(0))]);
            break;
        }
        a.on_tx_done(now + SECOND);
        now = a.next_deadline().expect("next burst or give-up");
        a.poll(now);
    }
    let levels: Vec<f64> = bursts.iter().map(|b| b.0.unwrap()).collect();
    assert_eq!(levels, fm::session::AUTOTUNE_LEVELS_DB);
    let steps: Vec<u8> = bursts
        .iter()
        .map(|b| Record0::parse(&b.1).unwrap())
        .map(|r| (r.kind == Kind::AutoTune) as u8 * r.snr)
        .collect();
    assert_eq!(steps, (1..=10).collect::<Vec<u8>>());
}

/// Two xara: one runs Autotune, the other answers; the caller learns step 4 (first step heard, 1, plus 3).
#[test]
fn autotune_between_two_xara() {
    let mut a = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N0CALL"));
    let mut b = fm::Session::new(fm::Config::new(FmBandwidth::Wide, "N1CALL"));
    drain(&mut a);
    drain(&mut b);
    assert!(a.probe(0, "N0CALL", "N1CALL", &[], true));
    let mut now: Time = 0;
    let mut got = Vec::new();
    for _ in 0..30 {
        for (_, f) in sent(&drain(&mut a)) {
            b.on_record0(now + SECOND / 2, &f, 25.0);
        }
        a.on_tx_done(now + SECOND);
        for (_, f) in sent(&drain(&mut b)) {
            b.on_tx_done(now + SECOND);
            a.on_record0(now + 2 * SECOND, &f, 25.0);
        }
        got.extend(probes(&drain(&mut a)));
        if !got.is_empty() {
            break;
        }
        now = a.next_deadline().expect("next burst");
        a.poll(now);
    }
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].0.as_str(), got[0].2), ("N1CALL", Some(4)));
}
