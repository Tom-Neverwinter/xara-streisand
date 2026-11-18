// A web page can make the operator's browser send an HTTP request to any
// port on 127.0.0.1.  These ports must not act on what follows the headers.

use std::io::Write;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use xara_host::http_request_line;
use xara_host::kiss::{FEND, KissServer};

const HEADERS: &[u8] = b"POST / HTTP/1.1\r\nHost: 127.0.0.1:8100\r\nContent-Type: text/plain\r\n\r\n";

#[test]
fn request_lines_are_recognised() {
    for yes in
        [&b"POST / HTTP/1.1\r\nHost: x"[..], b"GET /index.html HTTP/1.0\r\n", b"OPTIONS * HTTP/1.1", b"PUT /x HTTP/2\n"]
    {
        assert!(http_request_line(yes), "{:?}", String::from_utf8_lossy(yes));
    }
    for no in [
        &b""[..],
        b"CQFRAME N0CALL 2300\r",
        b"CONNECT N0CALL N1CALL\r",
        b"CONNECT N0CALL N1CALL VIA N2CALL\r",
        b"MYCALL N0CALL\r",
        b"GET\r",
        b"post / HTTP/1.1\r",
        b"POST  HTTP/1.1\r",
        b"POST / FTP/1.1\r",
        &[FEND, 0x00, b'P', b'O', b'S', b'T', FEND],
        &[0xff, 0xfe, b' ', b'/'],
    ] {
        assert!(!http_request_line(no), "{:?}", String::from_utf8_lossy(no));
    }
}

fn kiss_client(k: &KissServer) -> TcpStream {
    let c = TcpStream::connect(k.addr).unwrap();
    let t0 = Instant::now();
    while !k.has_clients() {
        assert!(t0.elapsed() < Duration::from_secs(2), "client never attached");
    }
    c
}

#[test]
fn kiss_ignores_a_browser_post_carrying_a_frame() {
    let k = KissServer::bind("127.0.0.1", 0).unwrap();
    let mut c = kiss_client(&k);
    let frame = [&[FEND, 0x02][..], b"spoofed payload from a web page", &[FEND]].concat();
    c.write_all(&[HEADERS, &frame].concat()).unwrap();
    assert!(k.input.recv_timeout(Duration::from_millis(500)).is_err(), "a frame from an HTTP request got through");
    let t0 = Instant::now();
    while k.has_clients() {
        assert!(t0.elapsed() < Duration::from_secs(2), "the HTTP client was not dropped");
    }
}

#[test]
fn kiss_still_takes_a_real_client() {
    let k = KissServer::bind("127.0.0.1", 0).unwrap();
    let mut c = kiss_client(&k);
    c.write_all(&[&[FEND, 0x02][..], b"hello", &[FEND]].concat()).unwrap();
    assert_eq!(k.input.recv_timeout(Duration::from_secs(1)).unwrap(), (2, b"hello".to_vec()));
}

#[cfg(unix)]
mod tnc_ports {
    use super::*;
    use xara_host::server::{Host, Input};

    fn next(h: &Host) -> Option<Input> {
        h.input.recv_timeout(Duration::from_millis(700)).ok()
    }

    #[test]
    fn command_port_runs_nothing_from_a_browser_post() {
        let h = Host::bind("127.0.0.1", 0).unwrap();
        let mut c = TcpStream::connect(h.cmd_addr).unwrap();
        assert!(matches!(next(&h), Some(Input::Connected)));
        c.write_all(&[HEADERS, b"\rCQFRAME N0CALL 2300\rTUNE\r"].concat()).unwrap();
        let mut seen = Vec::new();
        while let Some(i) = next(&h) {
            let closed = matches!(i, Input::Closed);
            seen.push(format!("{i:?}"));
            if closed {
                break;
            }
        }
        assert_eq!(seen.last().map(String::as_str), Some("Closed"), "connection not closed: {seen:?}");
        assert!(!seen.iter().any(|s| s.starts_with("Command") || s.starts_with("Wrong")), "acted on: {seen:?}");
    }

    #[test]
    fn command_port_still_takes_a_real_client() {
        let h = Host::bind("127.0.0.1", 0).unwrap();
        let mut c = TcpStream::connect(h.cmd_addr).unwrap();
        assert!(matches!(next(&h), Some(Input::Connected)));
        c.write_all(b"BW2300\r").unwrap();
        assert!(matches!(next(&h), Some(Input::Command(_))));
    }

    #[test]
    fn data_port_forwards_nothing_from_a_browser_post() {
        let h = Host::bind("127.0.0.1", 0).unwrap();
        let mut c = TcpStream::connect(h.data_addr).unwrap();
        assert!(matches!(next(&h), Some(Input::Connected)));
        c.write_all(&[HEADERS, b"payload a web page wants sent over the air"].concat()).unwrap();
        let mut seen = Vec::new();
        while let Some(i) = next(&h) {
            let closed = matches!(i, Input::Closed);
            seen.push(format!("{i:?}"));
            if closed {
                break;
            }
        }
        assert_eq!(seen.last().map(String::as_str), Some("Closed"), "connection not closed: {seen:?}");
        assert!(!seen.iter().any(|s| s.starts_with("Data")), "forwarded: {seen:?}");
    }

    #[test]
    fn data_port_still_forwards_real_data() {
        let h = Host::bind("127.0.0.1", 0).unwrap();
        let mut c = TcpStream::connect(h.data_addr).unwrap();
        assert!(matches!(next(&h), Some(Input::Connected)));
        c.write_all(b";FW: N0CALL\r").unwrap();
        assert!(matches!(next(&h), Some(Input::Data(d)) if d == b";FW: N0CALL\r"));
    }
}
