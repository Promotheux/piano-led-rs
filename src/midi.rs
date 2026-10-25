//! MIDI input via `midir`.
//!
//! The callback runs on midir's backend thread (ALSA sequencer on the Pi). It
//! decodes the raw message in place and writes to [`NoteState`] atomics: no
//! allocation, no locks, no syscalls except an optional futex wake of the
//! render thread on note-on so the LED reacts without waiting for the next
//! frame tick.

use std::sync::Arc;
use std::thread::Thread;

use midir::{Ignore, MidiIO, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::notes::NoteState;

const CLIENT_NAME: &str = "plv";

/// Decodes one channel-voice message. Returns `true` when a note was struck.
#[inline]
pub fn handle_message(msg: &[u8], state: &NoteState) -> bool {
    let [status, rest @ ..] = msg else { return false };
    match (status & 0xf0, rest) {
        (0x90, &[note, vel, ..]) if vel > 0 => {
            state.note_on(note, vel);
            true
        }
        // Note-on with velocity 0 is the common running-status note-off.
        (0x90, &[note, _, ..]) | (0x80, &[note, ..]) => {
            state.note_off(note);
            false
        }
        // The sustain pedal (CC 64) is deliberately ignored: LEDs follow the keys only.
        // CC 120 All Sound Off / 123 All Notes Off.
        (0xb0, &[120 | 123, ..]) => {
            state.all_off();
            false
        }
        _ => false,
    }
}

pub fn list_ports() -> Result<Vec<String>, String> {
    let input = MidiInput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect())
}

/// Picks the port whose name contains `filter` (case-insensitive), or else the
/// first port that isn't the ALSA "Midi Through" loopback or ourselves.
fn pick_port<T: MidiIO>(io: &T, filter: Option<&str>) -> Option<(T::Port, String)> {
    let filter = filter.map(str::to_lowercase);
    io.ports().into_iter().find_map(|p| {
        let name = io.port_name(&p).ok()?;
        let lname = name.to_lowercase();
        let ok = match &filter {
            Some(f) => lname.contains(f.as_str()),
            None => !lname.contains("midi through") && !lname.starts_with(CLIENT_NAME),
        };
        ok.then_some((p, name))
    })
}

/// A live MIDI connection. Dropping it closes the port.
pub struct MidiListener {
    _conn: MidiInputConnection<()>,
    pub port_name: String,
}

impl MidiListener {
    /// Connects to a matching port. `Ok(None)` means no port is available yet
    /// (the caller retries — keyboards are often powered on after the Pi).
    pub fn connect(
        filter: Option<&str>,
        state: Arc<NoteState>,
        render_thread: Option<Thread>,
    ) -> Result<Option<Self>, String> {
        let mut input = MidiInput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
        // Drop SysEx, clock and active-sensing in the backend so they never
        // reach the callback (clock alone is 24 msgs per beat).
        input.ignore(Ignore::All);
        let Some((port, name)) = pick_port(&input, filter) else {
            return Ok(None);
        };
        let conn = input
            .connect(
                &port,
                "plv-in",
                move |_ts, msg, _| {
                    if handle_message(msg, &state) {
                        if let Some(t) = &render_thread {
                            t.unpark();
                        }
                    }
                },
                (),
            )
            .map_err(|e| format!("connect to '{name}': {e}"))?;
        Ok(Some(MidiListener { _conn: conn, port_name: name }))
    }

    /// Whether our port is still enumerated. midir's ALSA backend doesn't
    /// signal unplugs, so the supervisor polls this (off the hot path).
    pub fn still_present(&self) -> bool {
        match MidiInput::new(CLIENT_NAME) {
            Ok(i) => i
                .ports()
                .iter()
                .any(|p| i.port_name(p).map(|n| n == self.port_name).unwrap_or(false)),
            Err(_) => true, // can't tell; don't tear down a working connection
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        let s = NoteState::new();
        assert!(handle_message(&[0x93, 60, 90], &s));
        assert_eq!(s.held(60), 90);
        assert!(!handle_message(&[0x93, 60, 0], &s));
        assert_eq!(s.held(60), 0);
        handle_message(&[0x90, 61, 10], &s);
        handle_message(&[0x80, 61, 64], &s);
        assert_eq!(s.held(61), 0);
        // Sustain pedal must not keep released keys alive.
        handle_message(&[0xb0, 64, 127], &s);
        assert_eq!(s.held(61), 0);
        assert_eq!(s.presses(60), 1);
        // Truncated / garbage input must not panic.
        assert!(!handle_message(&[], &s));
        assert!(!handle_message(&[0x90, 60], &s));
        assert!(!handle_message(&[0xf8], &s));
    }
}

/// MIDI output to the instrument (the player's auto-played notes).
pub struct MidiOut {
    conn: MidiOutputConnection,
    pub port_name: String,
}

impl MidiOut {
    pub fn connect(filter: Option<&str>) -> Result<Option<Self>, String> {
        let output = MidiOutput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
        let Some((port, name)) = pick_port(&output, filter) else {
            return Ok(None);
        };
        let conn = output
            .connect(&port, "plv-out")
            .map_err(|e| format!("connect output '{name}': {e}"))?;
        Ok(Some(MidiOut { conn, port_name: name }))
    }

    pub fn send(&mut self, msg: &[u8]) {
        // A dropped note is better than stalling the player on a dead port.
        let _ = self.conn.send(msg);
    }
}
