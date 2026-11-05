//! Records what you play to a MIDI file in the song library.
//!
//! Polls the key state from the player thread (every ~3 ms while recording),
//! so the MIDI callback stays allocation- and lock-free. Records your keys
//! and the sustain pedal; the player's own notes aren't included.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde::Serialize;

use crate::notes::NoteState;

/// 120 bpm at 480 ticks per quarter: one tick is 500/480 ms.
const TPQ: u16 = 480;
const TEMPO_US: u32 = 500_000;
/// About an hour of busy playing; past this, new events are dropped.
const MAX_EVENTS: usize = 200_000;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub recording: bool,
    pub ms: u32,
    /// Notes recorded so far.
    pub notes: u32,
}

struct Rec {
    active: bool,
    /// Take a snapshot on the next tick: keys already down don't count.
    fresh: bool,
    ms: f64,
    events: Vec<(u32, [u8; 3])>,
    notes: u32,
    on: [bool; 128],
    presses: [u8; 128],
    pedal: u8,
}

pub struct Recorder {
    dir: PathBuf,
    state: Mutex<Rec>,
}

impl Recorder {
    pub fn new(dir: PathBuf) -> Self {
        Recorder {
            dir,
            state: Mutex::new(Rec {
                active: false,
                fresh: false,
                ms: 0.0,
                events: Vec::new(),
                notes: 0,
                on: [false; 128],
                presses: [0; 128],
                pedal: 0,
            }),
        }
    }

    pub fn status(&self) -> Status {
        let r = self.state.lock().unwrap();
        Status { recording: r.active, ms: r.ms as u32, notes: r.notes }
    }

    pub fn recording(&self) -> bool {
        self.state.lock().unwrap().active
    }

    pub fn start(&self) {
        let mut r = self.state.lock().unwrap();
        (r.active, r.fresh, r.ms, r.notes) = (true, true, 0.0, 0);
        r.events.clear();
    }

    /// Stops and saves. Returns the new song's file name, or `None` if
    /// nothing was played.
    pub fn stop(&self) -> Result<Option<String>, String> {
        let mut r = self.state.lock().unwrap();
        if !std::mem::take(&mut r.active) {
            return Ok(None);
        }
        let end = r.ms as u32;
        for k in 0..128u8 {
            if std::mem::take(&mut r.on[k as usize]) {
                r.events.push((end, [0x80, k, 0]));
            }
        }
        if r.pedal >= 64 {
            r.events.push((end, [0xb0, 64, 0]));
        }
        if r.notes == 0 {
            return Ok(None);
        }
        let bytes = to_midi(&r.events)?;
        r.events = Vec::new(); // give the memory back
        drop(r);
        let name = free_name(&self.dir);
        let path = self.dir.join(&name);
        let tmp = path.with_extension("mid.tmp");
        std::fs::write(&tmp, bytes)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("can't save {name}: {e}"))?;
        Ok(Some(name))
    }

    /// Advances by `dt` ms and records key and pedal changes since the last tick.
    pub fn tick(&self, dt: f64, notes: &NoteState) {
        let mut r = self.state.lock().unwrap();
        if !r.active {
            return;
        }
        if std::mem::take(&mut r.fresh) {
            for k in 0..128 {
                r.presses[k] = notes.presses(k);
                r.on[k] = false;
            }
            r.pedal = notes.pedal();
            return;
        }
        r.ms += dt;
        let t = r.ms as u32;
        for k in 0..128usize {
            let (held, presses) = (notes.held(k), notes.presses(k));
            if presses != r.presses[k] {
                // A new press (possibly of a key that was still down).
                r.presses[k] = presses;
                if std::mem::take(&mut r.on[k]) {
                    r.push(t, [0x80, k as u8, 0]);
                }
                // Pressed and let go between two ticks: velocity is gone already.
                r.push(t, [0x90, k as u8, if held == 0 { 64 } else { held }]);
                r.notes += 1;
                r.on[k] = true;
            } else if held == 0 && std::mem::take(&mut r.on[k]) {
                r.push(t, [0x80, k as u8, 0]);
            }
        }
        let pedal = notes.pedal();
        if pedal != r.pedal {
            r.pedal = pedal;
            r.push(t, [0xb0, 64, pedal]);
        }
    }
}

impl Rec {
    fn push(&mut self, t: u32, msg: [u8; 3]) {
        if self.events.len() < MAX_EVENTS {
            self.events.push((t, msg));
        }
    }
}

/// "Recording 1.mid", "Recording 2.mid", …: the first one not taken.
fn free_name(dir: &Path) -> String {
    (1..)
        .map(|n| format!("Recording {n}.mid"))
        .find(|name| !dir.join(name).exists())
        .unwrap()
}

/// One track at 120 bpm, events at their recorded times.
fn to_midi(events: &[(u32, [u8; 3])]) -> Result<Vec<u8>, String> {
    let ticks = |ms: u32| (ms as u64 * TPQ as u64 * 1000 / TEMPO_US as u64) as u32;
    let mut track = vec![TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(TEMPO_US))),
    }];
    let mut last = 0;
    for &(ms, [status, a, b]) in events {
        let now = ticks(ms);
        let message = match status & 0xf0 {
            0x90 => MidiMessage::NoteOn { key: u7::new(a), vel: u7::new(b) },
            0x80 => MidiMessage::NoteOff { key: u7::new(a), vel: u7::new(b) },
            _ => MidiMessage::Controller { controller: u7::new(a), value: u7::new(b) },
        };
        track.push(TrackEvent { delta: u28::new(now - last), kind: TrackEventKind::Midi { channel: u4::new(0), message } });
        last = now;
    }
    track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
    let smf = Smf { header: Header::new(Format::SingleTrack, Timing::Metrical(u15::new(TPQ))), tracks: vec![track] };
    let mut out = Vec::new();
    smf.write_std(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keys_and_pedal_and_plays_back_in_the_player() {
        let dir = std::env::temp_dir().join(format!("plv-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = Recorder::new(dir.clone());
        let notes = NoteState::new();
        notes.note_on(30, 100); // already down before recording: ignored
        rec.start();
        rec.tick(3.0, &notes);
        rec.tick(500.0, &notes);
        notes.note_on(60, 90);
        notes.set_pedal(127);
        rec.tick(10.0, &notes);
        notes.note_off(60);
        notes.note_off(30);
        rec.tick(250.0, &notes);
        notes.set_pedal(0);
        rec.tick(10.0, &notes);
        assert_eq!(rec.status().notes, 1);
        let name = rec.stop().unwrap().expect("saved");
        assert_eq!(name, "Recording 1.mid");
        let song = crate::player::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
        assert_eq!(song.notes.len(), 1);
        let n = &song.notes[0];
        assert_eq!((n.key, n.vel), (60, 90));
        assert!((505..=515).contains(&n.t) && (245..=255).contains(&n.dur), "t {} dur {}", n.t, n.dur);
        assert_eq!(song.pedal.len(), 2);
        // Nothing played: nothing saved.
        rec.start();
        rec.tick(3.0, &notes);
        assert_eq!(rec.stop().unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
