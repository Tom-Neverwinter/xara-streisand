//! VARA SAT 4.4.5's codebook against a recorded link between two real stations (lab, 2026-11-18: N0CALL calls
//! N1CALL, sends text, N1CALL quits). SAT sends one tone per symbol; these are the tones heard (46.875 Hz grid).
use xara_phy::call::call_hash;
use xara_phy::hf::ctrl::code::{BREAK, IDLE};
use xara_phy::hf::ctrl::{Codebook, Codeword, Fmt, Role};
use xara_phy::hf::level::Bandwidth;

fn tones(s: &str) -> Vec<u32> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn book(seed_call: &str, me: &str, role: Role) -> Codebook {
    let h = call_hash(me.as_bytes());
    Codebook::new(call_hash(seed_call.as_bytes()), [h; 5], role, Bandwidth::Sat)
}

fn heard(w: &Codeword) -> Vec<u32> {
    w.iter().map(|s| s[0]).collect()
}

#[test]
fn sat_codebook_matches_a_recorded_link() {
    let answerer = book("N1CALL", "N1CALL", Role::Answerer);
    let caller = book("N0CALL", "N0CALL", Role::Caller);
    let f = Fmt::F0;
    for (code, want) in [
        (1, "50 13 14 23 24 47 22 23 34 25 42 19 16 37 36 41 38 11 44 31 36 27 50 31 52 51 20 23 52 41 30"),
        (3, "16 11 36 37 26 31 34 21 50 19 28 37 12 19 38 43 26 37 12 23 16 17 38 25 22 35 42 25 28 45 46"),
        (14, "26 51 14 31 52 45 34 45 40 11 32 13 24 45 50 47 46 21 20 27 28 23 36 17 38 41 50 33 26 51 32"),
        (17, "32 19 48 43 28 51 24 27 26 37 46 31 20 27 26 17 20 39 30 41 26 17 38 51 52 49 38 15 14 33 18"),
    ] {
        assert_eq!(heard(answerer.tx(f, code)), tones(want), "answerer tx {code}");
    }
    assert_eq!(
        heard(answerer.rx(f, 26)),
        tones("44 33 20 19 12 19 22 13 32 49 24 17 22 13 40 21 40 51 14 49 18 21 28 43 28 39 50 45 38 11 14"),
        "connect request"
    );
    for (code, want) in [
        (13, "38 49 20 13 22 43 44 11 38 21 32 37 36 49 42 29 48 33 46 49 52 17 40 49 16 43 48 39 14 45 36"),
        (14, "36 47 14 19 46 37 20 15 30 27 40 21 38 43 14 37 42 43 22 13 34 43 20 29 42 17 30 39 40 49 38"),
    ] {
        assert_eq!(heard(answerer.rx(f, code)), tones(want), "caller tx {code}");
    }
    assert_eq!(
        heard(caller.tx(f, BREAK)),
        tones("26 27 26 45 44 13 22 45 52 33 44 13 50 13 32 33 50 21 46 31 48 31 38 37 24 23 22 17 38 47 28")
    );
    assert_eq!(
        heard(caller.rx(f, BREAK)),
        tones("50 19 20 37 52 33 34 27 18 21 32 43 12 47 50 43 26 43 24 51 14 19 44 25 18 31 26 33 20 25 42")
    );
    assert_eq!(
        heard(caller.rx(f, IDLE)),
        tones("48 21 22 37 22 13 34 11 40 17 28 41 32 23 44 41 26 41 52 33 48 41 42 21 16 25 26 11 40 31 38")
    );
}

/// The codebook's connect request and reply are the same codewords `sat.rs` builds on its own.
#[test]
fn codebook_agrees_with_the_connect_codewords() {
    for call in ["N1CALL", "N0CALL", "B2B"] {
        let answerer = book(call, call, Role::Answerer);
        assert_eq!(heard(answerer.rx(Fmt::F0, 26)), xara_phy::sat::connect_request(call), "request {call}");
        assert_eq!(heard(answerer.tx(Fmt::F0, 17)), xara_phy::sat::connect_reply(call), "reply {call}");
    }
}
