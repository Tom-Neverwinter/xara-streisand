//! Conformance against VARA's published "VARA Protocol Native TNC Commands" (EA5HVK, 2025-10-10).
//! Keep in step with docs/COMPATIBILITY.md: when a NOT_YET reply gets implemented this test fails
//! until it moves out of NOT_YET and the chart is updated.
use xara_host::command::parse;
use xara_host::response::{LinkWidth, Response};

/// Every APPLICATION -> VARA command in the document, in its documented form.
const DOC_COMMANDS: [&str; 22] = [
    "CONNECT N0CALL N1CALL",
    "CONNECT N0CALL N1CALL VIA N2CALL N3CALL",
    "LISTEN ON",
    "LISTEN OFF",
    "MYCALL N0CALL N0CALL-1 N0CALL-15 N0CALL-T N0CALL-R",
    "DISCONNECT",
    "ABORT",
    "COMPRESSION OFF",
    "COMPRESSION TEXT",
    "COMPRESSION FILES",
    "BW500",
    "BW2300",
    "BW2750",
    "CHAT ON",
    "CHAT OFF",
    "WINLINK SESSION",
    "P2P SESSION",
    "TUNE -5",
    "TUNE ?",
    "TUNE OFF",
    "CLEANTXBUFFER",
    "VERSION",
];

#[test]
fn every_documented_command_is_understood() {
    let missing: Vec<_> = DOC_COMMANDS.iter().filter(|c| parse(c).is_none()).collect();
    assert!(missing.is_empty(), "documented VARA commands xara answers WRONG to: {missing:?}");
}

/// Every reply xara can put on the command port, one sample each.
fn xara_replies() -> Vec<String> {
    let connected = |vias: Vec<String>, width| Response::Connected {
        caller: "N0CALL".into(),
        answerer: "N1CALL".into(),
        vias,
        width,
    };
    [
        Response::Ok,
        Response::Wrong,
        Response::Busy(true),
        Response::Busy(false),
        Response::Ptt(true),
        Response::Ptt(false),
        Response::Bitrate { level: 4, bps: 175, tx: true },
        Response::Pending,
        Response::CancelPending,
        connected(vec![], LinkWidth::Hz(2300)),
        connected(vec!["N2CALL".into()], LinkWidth::Wide),
        Response::Disconnected,
        Response::LinkRegistered(true),
        Response::LinkRegistered(false),
        Response::Buffer(0),
        Response::Sn(3.0),
        Response::IAmAlive,
        Response::Registered(Some(vec!["N0CALL".into()])),
        Response::EncryptionDisabled,
        Response::UnencryptedLink,
        Response::Tune(-5.0),
        Response::CleanTxBuffer { empty: true },
        Response::CleanTxBuffer { empty: false },
        // Decoded CQs go out as a plain line built by xara_arq::cq.
        Response::Line("CQFRAME N0CALL 2300".into()),
    ]
    .iter()
    .map(Response::to_string)
    .collect()
}

/// VARA -> APPLICATION replies in the document, spelled as real VARA 4.9.0 sends them where that
/// differs from the PDF (vectors/host: "CLEANTXBUFFER BUFFEREMPTY", "CLEANTXBUFFER FAILED ").
const DOC_REPLIES: [&str; 26] = [
    "CONNECTED",
    "DISCONNECTED",
    "PTT OFF",
    "PTT ON",
    "BUFFER",
    "PENDING",
    "CANCELPENDING",
    "BUSY OFF",
    "BUSY ON",
    "REGISTERED",
    "LINK REGISTERED",
    "LINK UNREGISTERED",
    "IAMALIVE",
    "MISSING SOUNDCARD",
    "CQFRAME",
    "SN",
    "BITRATE",
    "CLEANTXBUFFER BUFFEREMPTY",
    "CLEANTXBUFFER FAILED",
    "CLEANTXBUFFER OK",
    "ENCRYPTION DISABLED",
    "ENCRYPTION READY",
    "UNENCRYPTED LINK",
    "ENCRYPTED LINK",
    "OK",
    "WRONG",
];

/// Documented replies xara never sends yet. See docs/COMPATIBILITY.md for why each one is open.
const NOT_YET: [&str; 4] = ["MISSING SOUNDCARD", "CLEANTXBUFFER OK", "ENCRYPTION READY", "ENCRYPTED LINK"];

fn sent(replies: &[String], keyword: &str) -> bool {
    replies.iter().any(|r| r == keyword || r.starts_with(&format!("{keyword} ")))
}

#[test]
fn every_documented_reply_is_sent_or_listed_as_a_gap() {
    let replies = xara_replies();
    for k in DOC_REPLIES {
        match (sent(&replies, k), NOT_YET.contains(&k)) {
            (true, true) => panic!("{k:?} is implemented now: drop it from NOT_YET and update docs/COMPATIBILITY.md"),
            (false, false) => panic!("{k:?} is documented by VARA but xara never sends it: add it to NOT_YET"),
            _ => {}
        }
    }
}
