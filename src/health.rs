//! Connection health shown in the tablet app, so a freeze can be diagnosed
//! without SSH. Written off the hot paths: the MIDI supervisor, once a second
//! by the render loop, and by WebSocket threads as tablets come and go.

use std::sync::atomic::{AtomicU16, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

/// No frame-rate report for this long means the render loop stalled.
const FPS_STALE_MS: u64 = 2500;

pub struct Health {
    started: Instant,
    midi_in: Mutex<Option<String>>,
    midi_out: Mutex<Option<String>>,
    led_fps: AtomicU16,
    /// When `led_fps` was last written, in ms since `started`.
    fps_at_ms: AtomicU64,
    ws_clients: AtomicU16,
    /// RTP-MIDI sessions: (peer SSRC, peer name).
    rtp_peers: Mutex<Vec<(u32, String)>>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub midi_in: Option<String>,
    pub midi_out: Option<String>,
    /// Render loop iterations in the last second (frames are computed even
    /// when an idle frame isn't sent), so 0 means the render thread stalled.
    pub led_fps: u16,
    /// Tablets with an open live-state WebSocket.
    pub ws_clients: u16,
    /// Computers connected over network MIDI (RTP-MIDI).
    pub rtp_peers: Vec<String>,
}

impl Health {
    pub fn new() -> Self {
        Health {
            started: Instant::now(),
            midi_in: Mutex::new(None),
            midi_out: Mutex::new(None),
            led_fps: AtomicU16::new(0),
            fps_at_ms: AtomicU64::new(0),
            ws_clients: AtomicU16::new(0),
            rtp_peers: Mutex::new(Vec::new()),
        }
    }

    pub fn rtp_joined(&self, ssrc: u32, name: &str) {
        let mut peers = self.rtp_peers.lock().unwrap();
        peers.retain(|(s, n)| *s != ssrc && n != name); // a reconnect replaces the old session
        peers.push((ssrc, name.to_string()));
    }

    pub fn rtp_left(&self, ssrc: u32) {
        self.rtp_peers.lock().unwrap().retain(|(s, _)| *s != ssrc);
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn set_midi_in(&self, port: Option<String>) {
        *self.midi_in.lock().unwrap() = port;
    }

    pub fn set_midi_out(&self, port: Option<String>) {
        *self.midi_out.lock().unwrap() = port;
    }

    pub fn set_led_fps(&self, fps: u16) {
        self.led_fps.store(fps, Relaxed);
        self.fps_at_ms.store(self.now_ms(), Relaxed);
    }

    pub fn ws_opened(&self) {
        self.ws_clients.fetch_add(1, Relaxed);
    }

    pub fn ws_closed(&self) {
        self.ws_clients.fetch_sub(1, Relaxed);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            midi_in: self.midi_in.lock().unwrap().clone(),
            midi_out: self.midi_out.lock().unwrap().clone(),
            // A stalled render loop stops reporting, so a stale value reads as 0.
            led_fps: if self.now_ms().saturating_sub(self.fps_at_ms.load(Relaxed)) > FPS_STALE_MS {
                0
            } else {
                self.led_fps.load(Relaxed)
            },
            ws_clients: self.ws_clients.load(Relaxed),
            rtp_peers: self.rtp_peers.lock().unwrap().iter().map(|(_, n)| n.clone()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stalled_render_loop_reads_as_zero_fps() {
        let mut h = Health::new();
        h.set_led_fps(60);
        assert_eq!(h.snapshot().led_fps, 60);
        // Pretend the last report was 3 s ago.
        h.started -= Duration::from_secs(3);
        assert_eq!(h.snapshot().led_fps, 0);
    }
}
