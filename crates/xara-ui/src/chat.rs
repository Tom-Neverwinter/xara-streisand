//! The terminal page's link to the TNC: keyboard-to-keyboard text from the web UI.
//!
//! A browser cannot open raw TCP, so `/chat/ws` is bridged to the TNC's own host ports, as any
//! VARA client program connects: command port (CR-terminated lines), data port (bytes), and the
//! KISS port for unconnected CQ / ALL broadcasts.  Over the websocket:
//! - text `C <line>` from the page is a host command (only the few a terminal needs, see `allowed`);
//! - text `R <line>` to the page is a host reply, `E <text>` a bridge error;
//! - binary frames carry one channel byte (`DATA` or `KISS`) and then the bytes, both ways.
use crate::ws;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use xara_host::writer::Writer;

pub const DATA: u8 = 0;
pub const KISS: u8 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FRAME: usize = 64 * 1024;
const KEEPALIVE: Duration = Duration::from_secs(2);

/// Where the bridge finds the TNC: the addresses its host ports are actually bound to.
#[derive(Clone, Copy, Debug)]
pub struct HostPorts {
    pub cmd: SocketAddr,
    pub data: SocketAddr,
    pub kiss: Option<SocketAddr>,
}

/// Host commands the page may send: the connection and station commands from VARA's published TCP
/// command list that a keyboard terminal needs.  Nothing that changes registration or the public flag.
pub fn allowed(line: &str) -> bool {
    let l = line.to_ascii_uppercase();
    !line.contains(['\r', '\n'])
        && line.len() <= 128
        && (["MYCALL ", "CONNECT ", "COMPRESSION "].iter().any(|p| l.starts_with(p))
            || ["DISCONNECT", "ABORT", "LISTEN ON", "LISTEN OFF", "CHAT ON", "CHAT OFF", "BW500", "BW2300", "BW2750"]
                .contains(&l.as_str()))
}

fn pump(mut from: TcpStream, w: Arc<Writer>, chan: Option<u8>) {
    let mut buf = [0u8; 4096];
    let mut line = Vec::new();
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let ok = match chan {
            Some(c) => w.send(&ws::frame(ws::BINARY, &[&[c], &buf[..n]].concat())),
            None => {
                let mut ok = true;
                for &b in &buf[..n] {
                    if b == b'\r' {
                        let text = format!("R {}", String::from_utf8_lossy(&line));
                        ok &= w.send(&ws::frame(ws::TEXT, text.as_bytes()));
                        line.clear();
                    } else if b != b'\n' && line.len() < 1024 {
                        line.push(b);
                    }
                }
                ok
            }
        };
        if !ok {
            break;
        }
    }
    let _ = w.send(&ws::frame(ws::TEXT, b"E TNC connection closed"));
    if chan.is_none() {
        // the command port is the session: end the websocket so the page reconnects
        w.finish();
    }
}

fn open(addr: SocketAddr) -> io::Result<TcpStream> {
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    s.set_nodelay(true)?;
    Ok(s)
}

/// Serve one chat page: `s` is the upgraded websocket, `w` its writer.  Returns when either side closes.
pub(crate) fn serve(mut s: TcpStream, w: Arc<Writer>, ports: HostPorts) {
    let say = |t: String| w.send(&ws::frame(ws::TEXT, t.as_bytes()));
    let links = (open(ports.cmd), open(ports.data));
    let (Ok(cmd), Ok(data)) = links else {
        say(format!("E cannot reach the TNC at {}: is another client connected?", ports.cmd));
        return;
    };
    let kiss = ports.kiss.and_then(|k| open(k).ok());
    let mut socks = vec![(cmd.try_clone(), None), (data.try_clone(), Some(DATA))];
    if let Some(k) = &kiss {
        socks.push((k.try_clone(), Some(KISS)));
    }
    for (sock, chan) in socks {
        let (Ok(sock), w) = (sock, w.clone()) else { continue };
        let _ = thread::Builder::new().name("ui-chat".into()).spawn(move || pump(sock, w, chan));
    }
    say(format!("E connected to the TNC at {}{}", ports.cmd, if kiss.is_some() { " (with KISS)" } else { "" }));
    // the page only speaks when typed at; pings make its browser answer inside the read timeout
    let pinger = w.clone();
    let _ = thread::Builder::new().name("ui-chat-ping".into()).spawn(move || {
        while pinger.send(&ws::frame(ws::PING, b"")) {
            thread::sleep(KEEPALIVE);
        }
    });
    let (mut cmd_w, mut data_w) = (cmd.try_clone().ok(), data.try_clone().ok());
    let mut kiss_w = kiss.as_ref().and_then(|k| k.try_clone().ok());
    let mut rd = ws::Reader::default();
    while let Ok(f) = rd.next(&mut s, MAX_FRAME) {
        let sent = match f.op {
            ws::TEXT => match std::str::from_utf8(&f.payload).ok().and_then(|t| t.strip_prefix("C ")) {
                Some(line) if allowed(line) => cmd_w.as_mut().map(|c| c.write_all(format!("{line}\r").as_bytes())),
                Some(line) => {
                    say(format!("E command not allowed from the chat page: {line}"));
                    None
                }
                None => None,
            },
            ws::BINARY => match f.payload.split_first() {
                Some((&DATA, b)) => data_w.as_mut().map(|d| d.write_all(b)),
                Some((&KISS, b)) => kiss_w.as_mut().map(|k| k.write_all(b)),
                _ => None,
            },
            ws::PING => {
                w.send(&ws::frame(ws::PONG, &f.payload));
                None
            }
            ws::CLOSE => break,
            _ => None,
        };
        if matches!(sent, Some(Err(_))) {
            say("E write to the TNC failed".into());
            break;
        }
    }
    for t in [Some(&cmd), Some(&data), kiss.as_ref()].into_iter().flatten() {
        let _ = t.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// A masked client frame (mask key 0, so the payload is sent as is); payloads < 126 bytes.
    fn client_frame(op: u8, payload: &[u8]) -> Vec<u8> {
        [&[0x80 | op, 0x80 | payload.len() as u8, 0, 0, 0, 0][..], payload].concat()
    }

    /// An unmasked server frame; payloads < 126 bytes.
    /// The next server frame other than a keepalive ping.
    fn read_server_frame(r: &mut impl Read) -> (u8, Vec<u8>) {
        loop {
            let mut h = [0u8; 2];
            r.read_exact(&mut h).unwrap();
            let mut p = vec![0u8; usize::from(h[1] & 0x7f)];
            r.read_exact(&mut p).unwrap();
            if h[0] & 0x0f != ws::PING {
                return (h[0] & 0x0f, p);
            }
        }
    }

    #[test]
    fn bridges_commands_replies_and_data_to_the_tnc_ports() {
        // a fake TNC: its command and data ports
        let (cmd_l, data_l) = (TcpListener::bind("127.0.0.1:0").unwrap(), TcpListener::bind("127.0.0.1:0").unwrap());
        let ports = HostPorts { cmd: cmd_l.local_addr().unwrap(), data: data_l.local_addr().unwrap(), kiss: None };
        let ws_l = TcpListener::bind("127.0.0.1:0").unwrap();
        let ws_addr = ws_l.local_addr().unwrap();
        thread::spawn(move || {
            let (s, _) = ws_l.accept().unwrap();
            let w = Arc::new(Writer::spawn(&s).unwrap());
            serve(s, w, ports);
        });
        let mut page = TcpStream::connect(ws_addr).unwrap();
        let (mut tnc_cmd, _) = cmd_l.accept().unwrap();
        let (mut tnc_data, _) = data_l.accept().unwrap();
        let (op, hello) = read_server_frame(&mut page);
        assert_eq!(op, ws::TEXT);
        assert!(String::from_utf8_lossy(&hello).starts_with("E connected to the TNC"));

        page.write_all(&client_frame(ws::TEXT, b"C MYCALL N0CALL")).unwrap();
        page.write_all(&client_frame(ws::TEXT, b"C PUBLIC ON")).unwrap();
        page.write_all(&client_frame(ws::BINARY, b"\x00hello")).unwrap();
        let mut line = Vec::new();
        BufReader::new(tnc_cmd.try_clone().unwrap()).read_until(b'\r', &mut line).unwrap();
        assert_eq!(line, b"MYCALL N0CALL\r");
        let (_, refused) = read_server_frame(&mut page);
        assert_eq!(refused, b"E command not allowed from the chat page: PUBLIC ON");
        let mut got = [0u8; 5];
        tnc_data.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hello");

        tnc_cmd.write_all(b"CONNECTED N0CALL N1CALL 2300\r").unwrap();
        assert_eq!(read_server_frame(&mut page), (ws::TEXT, b"R CONNECTED N0CALL N1CALL 2300".to_vec()));
        tnc_data.write_all(b"hi back").unwrap();
        assert_eq!(read_server_frame(&mut page), (ws::BINARY, b"\x00hi back".to_vec()));

        // the TNC hangs up: the page is told, then its websocket ends so it reconnects
        drop(tnc_cmd);
        assert_eq!(read_server_frame(&mut page).1, b"E TNC connection closed");
        page.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut rest = Vec::new();
        let ended = page
            .read_to_end(&mut rest)
            .map_or_else(|e| e.kind() != io::ErrorKind::WouldBlock && e.kind() != io::ErrorKind::TimedOut, |_| true);
        assert!(ended, "websocket still open after the TNC closed");
    }

    #[test]
    fn only_terminal_commands_pass() {
        for ok in [
            "MYCALL N0CALL",
            "CONNECT N0CALL N1CALL",
            "CONNECT A B VIA C",
            "chat on",
            "BW2300",
            "COMPRESSION FILES",
            "ABORT",
        ] {
            assert!(allowed(ok), "{ok}");
        }
        for bad in ["PUBLIC ON", "MYCALL A\rABORT", "BW9999", "LISTEN", "REGISTERED", ""] {
            assert!(!allowed(bad), "{bad}");
        }
    }
}
