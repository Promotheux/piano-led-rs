//! Native RTP-MIDI (AppleMIDI) listener, so a PC (e.g. rtpMIDI on Windows)
//! can send MIDI to the Pi over Wi-Fi without a separate daemon.
//!
//! Two UDP sockets, one blocking thread each: session commands on `port`,
//! MIDI data on `port + 1`. We accept every invitation, answer clock syncs to
//! keep sessions alive, send receiver feedback, and feed note messages into
//! the same `handle_message` path as USB MIDI. [`RtpOut`] sends the player's
//! notes back to every connected computer. No recovery journal either way: a
//! lost note-off is caught by the renderer's stuck-note release.
//! Discovery is left to avahi (see `advertise`).

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU16, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::health::Health;
use crate::midi::handle_message;
use crate::notes::NoteState;

/// Name shown in the PC's session list.
pub const SESSION_NAME: &str = "Piano LED";
const SIGNATURE: [u8; 2] = [0xff, 0xff];
const AVAHI_FILE: &str = "/etc/avahi/services/plv-rtpmidi.service";

/// An AppleMIDI session command (the packets starting with 0xFFFF).
#[derive(Debug, PartialEq)]
enum Command<'a> {
    Invite { token: u32, ssrc: u32, name: &'a str },
    Sync { ssrc: u32, count: u8, ts1: u64 },
    Bye { ssrc: u32 },
    Other,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn parse_command(p: &[u8]) -> Option<Command<'_>> {
    if p.get(..2)? != SIGNATURE {
        return None;
    }
    Some(match p.get(2..4)? {
        b"IN" => {
            let name = p.get(16..).unwrap_or_default();
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            Command::Invite { token: be32(p, 8)?, ssrc: be32(p, 12)?, name: std::str::from_utf8(name).unwrap_or("?") }
        }
        b"CK" => Command::Sync { ssrc: be32(p, 4)?, count: *p.get(8)?, ts1: be64(p, 12)? },
        b"BY" => Command::Bye { ssrc: be32(p, 12)? },
        _ => Command::Other,
    })
}

/// Our answer to an invitation: protocol version 2, the caller's token, our id and name.
fn accept(token: u32, ssrc: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(16 + SESSION_NAME.len() + 1);
    p.extend_from_slice(&SIGNATURE);
    p.extend_from_slice(b"OK");
    p.extend_from_slice(&2u32.to_be_bytes());
    p.extend_from_slice(&token.to_be_bytes());
    p.extend_from_slice(&ssrc.to_be_bytes());
    p.extend_from_slice(SESSION_NAME.as_bytes());
    p.push(0);
    p
}

/// Clock sync reply (count 1): echo the caller's timestamp, add ours.
fn sync_reply(ssrc: u32, ts1: u64, now: u64) -> [u8; 36] {
    let mut p = [0u8; 36];
    p[..2].copy_from_slice(&SIGNATURE);
    p[2..4].copy_from_slice(b"CK");
    p[4..8].copy_from_slice(&ssrc.to_be_bytes());
    p[8] = 1;
    p[12..20].copy_from_slice(&ts1.to_be_bytes());
    p[20..28].copy_from_slice(&now.to_be_bytes());
    p
}

/// Receiver feedback: tells the sender it can drop journal data up to `seq`.
fn feedback(ssrc: u32, seq: u16) -> [u8; 12] {
    let mut p = [0u8; 12];
    p[..2].copy_from_slice(&SIGNATURE);
    p[2..4].copy_from_slice(b"RS");
    p[4..8].copy_from_slice(&ssrc.to_be_bytes());
    p[8..10].copy_from_slice(&seq.to_be_bytes());
    p
}

/// Data bytes after a status byte; `None` for SysEx (variable length).
fn data_len(status: u8) -> Option<usize> {
    Some(match status {
        0x80..=0xbf | 0xe0..=0xef | 0xf2 => 2,
        0xc0..=0xdf | 0xf1 | 0xf3 => 1,
        0xf0 => return None,
        _ => 0,
    })
}

/// Walks the MIDI list of an RTP-MIDI packet (RFC 6295) and calls `on_msg`
/// with each channel message as complete bytes (running status expanded).
/// Returns the RTP sequence number, or `None` if it isn't an RTP-MIDI packet.
fn parse_rtp_midi(p: &[u8], mut on_msg: impl FnMut(&[u8])) -> Option<u16> {
    if p.len() < 13 || p[0] >> 6 != 2 || p[1] & 0x7f != 0x61 {
        return None;
    }
    let seq = u16::from_be_bytes([p[2], p[3]]);
    let mut i = 12 + 4 * (p[0] & 0x0f) as usize; // skip CSRCs
    let flags = *p.get(i)?;
    let (len, header) = if flags & 0x80 != 0 {
        ((((flags & 0x0f) as usize) << 8) | *p.get(i + 1)? as usize, 2)
    } else {
        ((flags & 0x0f) as usize, 1)
    };
    i += header;
    let end = (i + len).min(p.len());
    let mut first = true;
    let mut running = 0u8;
    while i < end {
        // Every command but the first carries a delta time (the first too if Z is set).
        if !first || flags & 0x20 != 0 {
            // Variable-length quantity: up to 4 bytes, high bit = more follow.
            for _ in 0..4 {
                let Some(&b) = p.get(i).filter(|_| i < end) else { break };
                i += 1;
                if b & 0x80 == 0 {
                    break;
                }
            }
        }
        first = false;
        let Some(&b) = p.get(i).filter(|_| i < end) else { break };
        let status = if b & 0x80 != 0 {
            i += 1;
            if b < 0xf0 {
                running = b;
            } else if b < 0xf8 {
                running = 0; // system common messages cancel running status
            }
            b
        } else if running != 0 {
            running
        } else {
            break; // data byte with nothing to run on: give up on this packet
        };
        match data_len(status) {
            Some(n) => {
                let data = p.get(i..(i + n).min(end))?;
                if data.len() < n {
                    break;
                }
                if status < 0xf0 {
                    let mut msg = [status, 0, 0];
                    msg[1..=n].copy_from_slice(data);
                    on_msg(&msg[..=n]);
                }
                i += n;
            }
            // SysEx: skip to its end (F7) or segment marker (F0/F4).
            None => i += p[i..end].iter().position(|&c| matches!(c, 0xf7 | 0xf0 | 0xf4)).map_or(end - i, |k| k + 1),
        }
    }
    Some(seq)
}

/// One RTP-MIDI packet carrying a single MIDI message (no journal).
fn rtp_packet(ssrc: u32, seq: u16, timestamp: u32, msg: &[u8]) -> ([u8; 16], usize) {
    let mut p = [0u8; 16];
    p[0] = 0x80; // RTP version 2
    p[1] = 0x61; // payload type 97, as AppleMIDI uses
    p[2..4].copy_from_slice(&seq.to_be_bytes());
    p[4..8].copy_from_slice(&timestamp.to_be_bytes());
    p[8..12].copy_from_slice(&ssrc.to_be_bytes());
    let n = msg.len().min(3);
    p[12] = n as u8; // short header: no journal, no delta time, length
    p[13..13 + n].copy_from_slice(&msg[..n]);
    (p, 13 + n)
}

/// Sends MIDI to every computer in a session (their data ports).
pub struct RtpOut {
    socket: UdpSocket,
    ssrc: u32,
    start: Instant,
    seq: AtomicU16,
    /// (peer SSRC, peer data address). Learned from invitations and from the
    /// clock syncs they keep sending, so a restart of plv finds them again.
    peers: Mutex<Vec<(u32, SocketAddr)>>,
}

impl RtpOut {
    /// Remembers a peer's data address; `true` if it wasn't known yet.
    fn learn(&self, ssrc: u32, addr: SocketAddr) -> bool {
        let mut peers = self.peers.lock().unwrap();
        match peers.iter_mut().find(|(s, _)| *s == ssrc) {
            Some(p) => {
                p.1 = addr;
                false
            }
            None => {
                peers.push((ssrc, addr));
                true
            }
        }
    }

    fn forget(&self, ssrc: u32) {
        self.peers.lock().unwrap().retain(|(s, _)| *s != ssrc);
    }

    /// Sends one MIDI message to every connected computer.
    pub fn send(&self, msg: &[u8]) {
        let peers = self.peers.lock().unwrap();
        if peers.is_empty() {
            return;
        }
        let (pkt, len) = rtp_packet(self.ssrc, self.seq.fetch_add(1, Relaxed), clock(self.start) as u32, msg);
        for (_, addr) in peers.iter() {
            let _ = self.socket.send_to(&pkt[..len], addr);
        }
    }
}

/// Time in 100 µs units, AppleMIDI's clock.
fn clock(start: Instant) -> u64 {
    (start.elapsed().as_micros() / 100) as u64
}

/// Starts the listener and returns the sender for the player. Errors (port
/// taken, e.g. by rtpmidid) are returned so the caller can log them and carry
/// on with USB MIDI.
pub fn spawn(port: u16, notes: Arc<NoteState>, render: Thread, health: Arc<Health>) -> Result<Arc<RtpOut>, String> {
    let bind = |p: u16| UdpSocket::bind(("0.0.0.0", p)).map_err(|e| format!("rtp-midi: port {p}: {e} (is rtpmidid running?)"));
    let control = bind(port)?;
    let data = bind(port + 1)?;
    let feedback_socket = control.try_clone().map_err(|e| e.to_string())?;
    let ssrc = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) ^ std::process::id();
    let start = Instant::now();
    let out = Arc::new(RtpOut {
        socket: data.try_clone().map_err(|e| e.to_string())?,
        ssrc,
        start,
        seq: AtomicU16::new(0),
        peers: Mutex::new(Vec::new()),
    });
    let out_control = out.clone();
    let out_data = out.clone();
    let health_data = health.clone();

    let h = health.clone();
    std::thread::Builder::new()
        .name("plv-rtp-control".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = control.recv_from(&mut buf) {
                match parse_command(&buf[..n]) {
                    Some(Command::Invite { token, ssrc: peer, name }) => {
                        let _ = control.send_to(&accept(token, ssrc), from);
                        eprintln!("rtp-midi: session from '{name}' ({from})");
                        h.rtp_joined(peer, name);
                    }
                    Some(Command::Bye { ssrc: peer }) => {
                        h.rtp_left(peer);
                        out_control.forget(peer);
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;

    std::thread::Builder::new()
        .name("plv-rtp-data".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut buf = [0u8; 1500];
            let mut last_feedback = Instant::now();
            while let Ok((n, from)) = data.recv_from(&mut buf) {
                let pkt = &buf[..n];
                match parse_command(pkt) {
                    Some(Command::Invite { token, ssrc: peer, .. }) => {
                        let _ = data.send_to(&accept(token, ssrc), from);
                        out_data.learn(peer, from);
                    }
                    Some(Command::Sync { ssrc: peer, count, ts1 }) => {
                        // A session that outlived a restart of plv: known again by its clock syncs.
                        if out_data.learn(peer, from) {
                            health_data.rtp_joined(peer, &from.ip().to_string());
                        }
                        if count == 0 {
                            let _ = data.send_to(&sync_reply(ssrc, ts1, clock(start)), from);
                        }
                    }
                    Some(_) => {}
                    None => {
                        let Some(seq) = parse_rtp_midi(pkt, |msg| {
                            if handle_message(msg, &notes) {
                                render.unpark();
                            }
                        }) else {
                            continue;
                        };
                        // Feedback goes to the sender's control port (data port - 1).
                        if last_feedback.elapsed() > Duration::from_secs(1) {
                            let to = SocketAddr::new(from.ip(), from.port().saturating_sub(1));
                            let _ = feedback_socket.send_to(&feedback(ssrc, seq), to);
                            last_feedback = Instant::now();
                        }
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Lets avahi announce the session over mDNS, so the PC finds "Piano LED"
/// by itself (`None` withdraws it). Only where avahi is installed (Raspberry
/// Pi OS); needs root.
pub fn advertise(port: Option<u16>) {
    let dir = std::path::Path::new(AVAHI_FILE).parent().unwrap();
    if !dir.is_dir() {
        return;
    }
    let Some(port) = port else {
        let _ = std::fs::remove_file(AVAHI_FILE);
        return;
    };
    let xml = format!(
        "<?xml version=\"1.0\" standalone='no'?>\n<!DOCTYPE service-group SYSTEM \"avahi-service.dtd\">\n\
         <!-- Written by plv at start-up -->\n<service-group>\n  <name>{SESSION_NAME}</name>\n  \
         <service>\n    <type>_apple-midi._udp</type>\n    <port>{port}</port>\n  </service>\n</service-group>\n"
    );
    if std::fs::read_to_string(AVAHI_FILE).ok().as_deref() != Some(xml.as_str()) {
        if let Err(e) = std::fs::write(AVAHI_FILE, xml) {
            eprintln!("rtp-midi: can't write {AVAHI_FILE}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_and_answer() {
        let mut inv = vec![0xff, 0xff, b'I', b'N', 0, 0, 0, 2, 0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 7];
        inv.extend_from_slice(b"CHESTNUT\0");
        assert_eq!(parse_command(&inv), Some(Command::Invite { token: 0xdeadbeef, ssrc: 7, name: "CHESTNUT" }));
        let ok = accept(0xdeadbeef, 42);
        assert_eq!(&ok[..4], b"\xff\xffOK");
        assert_eq!(&ok[8..16], &[0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 42]);
        assert!(ok.ends_with(b"Piano LED\0"));
        assert_eq!(parse_command(b"\x90\x61\x00\x01"), None, "not a command");
    }

    #[test]
    fn clock_sync_echoes_timestamp() {
        let mut ck = vec![0xff, 0xff, b'C', b'K', 0, 0, 0, 9, 0, 0, 0, 0];
        ck.extend_from_slice(&1234u64.to_be_bytes());
        ck.extend_from_slice(&[0; 16]);
        assert_eq!(parse_command(&ck), Some(Command::Sync { ssrc: 9, count: 0, ts1: 1234 }));
        let r = sync_reply(5, 1234, 999);
        assert_eq!(r[8], 1);
        assert_eq!(be64(&r, 12), Some(1234));
        assert_eq!(be64(&r, 20), Some(999));
    }

    fn rtp(midi: &[u8], flags: u8) -> Vec<u8> {
        let mut p = vec![0x80, 0x61, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 1];
        // Lengths over 15 need the long (B) header, like a real sender uses.
        if midi.len() > 15 {
            p.extend_from_slice(&[0x80 | flags | (midi.len() >> 8) as u8, midi.len() as u8]);
        } else {
            p.push(flags | midi.len() as u8);
        }
        p.extend_from_slice(midi);
        p
    }

    #[test]
    fn parses_midi_list_with_deltas_and_running_status() {
        let mut got = Vec::new();
        // Note on 60, delta 0 + running-status note on 64, delta + note off 60,
        // delta + clock (realtime, ignored), delta + CC 64.
        let midi = [0x90, 60, 100, 0x00, 64, 90, 0x05, 0x80, 60, 0, 0x01, 0xf8, 0x00, 0xb0, 64, 127];
        let seq = parse_rtp_midi(&rtp(&midi, 0), |m| got.push(m.to_vec()));
        assert_eq!(seq, Some(0x1234));
        assert_eq!(got, vec![vec![0x90, 60, 100], vec![0x90, 64, 90], vec![0x80, 60, 0], vec![0xb0, 64, 127]]);
    }

    #[test]
    fn sent_packets_parse_back() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let out = RtpOut {
            socket: UdpSocket::bind("127.0.0.1:0").unwrap(),
            ssrc: 1,
            start: Instant::now(),
            seq: AtomicU16::new(7),
            peers: Mutex::new(Vec::new()),
        };
        out.send(&[0x90, 60, 100]); // no peers yet: nothing sent
        out.learn(42, rx.local_addr().unwrap());
        out.learn(42, rx.local_addr().unwrap()); // re-learning doesn't duplicate
        out.send(&[0x90, 60, 100]);
        out.send(&[0xb0, 64, 0]);
        let mut buf = [0u8; 64];
        for want in [vec![0x90, 60, 100], vec![0xb0, 64, 0]] {
            let n = rx.recv(&mut buf).unwrap();
            let mut got = Vec::new();
            assert!(parse_rtp_midi(&buf[..n], |m| got.push(m.to_vec())).is_some());
            assert_eq!(got, vec![want]);
        }
        out.forget(42);
        assert!(out.peers.lock().unwrap().is_empty());
    }

    #[test]
    fn z_flag_long_header_and_sysex() {
        let mut got = Vec::new();
        // Z: the first command has a delta time (two bytes here); SysEx is skipped.
        let midi = [0x81, 0x00, 0x90, 61, 1, 0x00, 0xf0, 1, 2, 3, 0xf7, 0x00, 0x91, 62, 2];
        assert!(parse_rtp_midi(&rtp(&midi, 0x20), |m| got.push(m.to_vec())).is_some());
        assert_eq!(got, vec![vec![0x90, 61, 1], vec![0x91, 62, 2]]);
        // Long (B) header: 12-bit length.
        let mut p = vec![0x80, 0x61, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0x80, 3, 0x90, 70, 80];
        got.clear();
        assert!(parse_rtp_midi(&p, |m| got.push(m.to_vec())).is_some());
        assert_eq!(got, vec![vec![0x90, 70, 80]]);
        // Truncated or foreign packets don't panic.
        p.truncate(15);
        let _ = parse_rtp_midi(&p, |_| {});
        assert_eq!(parse_rtp_midi(&[0u8; 20], |_| {}), None);
    }
}
