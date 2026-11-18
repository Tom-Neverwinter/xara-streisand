use xara_phy::sat::{SYMBOL_SAMPLES, connect_reply, connect_request, preamble, reply_tones, request_tones, synth};

/// Connect-request codewords measured from real VARA SAT 4.4.5 in a lab loopback (2026-11-18), one per target call.
/// The source call did not change them (N0CALL, N0CALM and N0CALL-7 calling N1CALL all sent the first row).
const LAB: [(&str, &str); 6] = [
    ("N1CALL", "44 33 20 19 12 19 22 13 32 49 24 17 22 13 40 21 40 51 14 49 18 21 28 43 28 39 50 45 38 11 14"),
    ("N1CALM", "46 19 30 51 20 27 46 51 16 33 32 29 32 43 40 19 32 21 40 47 20 47 34 33 20 47 30 45 34 13 44"),
    ("N1CALK", "40 15 24 49 44 17 18 23 42 23 46 13 22 31 12 11 38 49 44 13 28 13 26 51 38 33 26 47 30 21 34"),
    ("N2CALL", "14 11 34 31 16 17 52 37 16 45 40 15 16 11 38 33 24 47 20 35 36 41 36 19 36 39 30 39 36 39 20"),
    ("B2B", "16 17 44 37 12 43 24 21 12 13 18 11 36 21 34 23 30 15 48 35 26 25 50 19 30 31 36 47 46 51 44"),
    ("N1CALL-1", "46 43 14 13 52 35 22 27 44 39 44 19 24 51 34 19 24 33 14 23 14 25 34 23 28 37 44 49 46 51 28"),
];

fn tones(s: &str) -> Vec<u32> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

#[test]
fn connect_requests_match_vara_sat() {
    for (call, want) in LAB {
        assert_eq!(connect_request(call), tones(want), "{call}");
    }
    assert_eq!(connect_request("n1call"), connect_request("N1CALL"));
}

#[test]
fn calls_start_like_vara_sat() {
    assert_eq!(preamble(), tones("40 34 36 26 26 43 16 42 44 40"));
    let first = request_tones("N1CALL", true);
    assert_eq!((first.len(), &first[..28], &first[28..]), (59, &[32; 28][..], &tones(LAB[0].1)[..]));
    assert_eq!(request_tones("N1CALL", false).len(), 41);
}

/// The audio carries each tone for one whole symbol: the strongest DFT bin of every symbol is that tone.
#[test]
fn synth_puts_one_tone_in_each_symbol() {
    let t = request_tones("N2CALL", false);
    let x = synth(&t);
    assert_eq!(x.len(), t.len() * SYMBOL_SAMPLES);
    for (i, &k) in t.iter().enumerate() {
        let s = &x[i * SYMBOL_SAMPLES..(i + 1) * SYMBOL_SAMPLES];
        let power = |bin: u32| {
            let w = 2.0 * std::f64::consts::PI * f64::from(bin) / SYMBOL_SAMPLES as f64;
            let (c, q) = s
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(c, q), (n, v)| (c + v * (w * n as f64).cos(), q + v * (w * n as f64).sin()));
            c * c + q * q
        };
        assert_eq!((8..60).max_by(|&a, &b| power(a).total_cmp(&power(b))), Some(k), "symbol {i}");
    }
}

/// A request is found for its own call, at the right sample, through noise; not for another call.
#[test]
fn finds_a_request_for_its_call_only() {
    use xara_phy::sat::{PREAMBLE_SYMBOLS, find_request};
    let mut x = vec![0.0; 3000];
    x.extend(synth(&request_tones("N1CALL", false)));
    x.extend(vec![0.0; 2000]);
    let mut seed = 1u32;
    for v in &mut x {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *v = *v * 0.3 + (f64::from(seed >> 16) / 32768.0 - 1.0) * 0.5; // about -6 dB S/N in the 3 kHz band
    }
    let (at, score) = find_request(&x, "N1CALL", 0.3).expect("found");
    let want = 3000 + PREAMBLE_SYMBOLS * SYMBOL_SAMPLES;
    assert!(at.abs_diff(want) <= SYMBOL_SAMPLES / 8, "at {at}, want {want}, score {score}");
    assert_eq!(find_request(&x, "N1CALM", 0.3), None);
}

/// Replies real VARA SAT 4.4.5 sent to xara's synthesized requests in the lab (2026-11-18), keyed by its own call.
#[test]
fn connect_replies_match_vara_sat() {
    for (call, want) in [
        ("N1CALL", "32 19 48 43 28 51 24 27 26 37 46 31 20 27 26 17 20 39 30 41 26 17 38 51 52 49 38 15 14 33 18"),
        ("N1CALM", "22 29 28 23 34 41 48 11 18 37 48 31 24 15 16 47 40 33 44 39 36 13 20 35 48 47 30 13 30 19 26"),
        ("B2B", "40 13 34 15 30 33 30 35 30 29 48 23 14 15 24 29 28 21 34 37 20 47 50 11 34 17 16 43 20 33 20"),
    ] {
        assert_eq!(connect_reply(call), tones(want), "{call}");
    }
    assert_eq!(reply_tones("N1CALL", false)[..10], preamble()[..]);
}
