//! Lock-free note state shared between the MIDI thread and the renderer.
//!
//! The MIDI callback must never block (a stalled ALSA callback drops events),
//! so instead of a mutex-protected buffer or a channel we use plain atomics:
//!
//! * `held[n]`   – current velocity of note `n`, 0 when the key is up.
//! * `struck[n]` – latched velocity of the most recent note-on since the
//!   renderer last looked. The renderer `swap`s it back to 0, so a key tapped
//!   and released between two frames is still drawn.
//! * `presses[n]` – wrapping count of note-ons, so the player can require a
//!   fresh press instead of accepting a key that was never released.
//! * `hint[n]`   – player overlay: which hand should play `n` ([`HINT_RIGHT`] /
//!   [`HINT_LEFT`]), optionally with [`HINT_BRIGHT`] when the player itself is
//!   sounding the note. `hint_struck[n]` latches each time the player
//!   sounds `n`, like `struck` does for keys you play. [`HINT_BLINK`] marks
//!   keys the player has been waiting on for too long.
//! * `practice`  – a practice mode is running: keys you press are shown
//!   green (expected) or red (wrong) instead of the normal key colour.
//!
//! All fixed-size, no heap, no locks; a write is a single relaxed store.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Relaxed};

pub const HINT_RIGHT: u8 = 1;
pub const HINT_LEFT: u8 = 2;
pub const HINT_BRIGHT: u8 = 4;
pub const HINT_BLINK: u8 = 8;
/// The next notes after the ones to play now (wait mode), shown faintly.
pub const HINT_PREVIEW: u8 = 16;

pub struct NoteState {
    held: [AtomicU8; 128],
    struck: [AtomicU8; 128],
    hint: [AtomicU8; 128],
    hint_struck: [AtomicBool; 128],
    presses: [AtomicU8; 128],
    practice: AtomicBool,
    playing: AtomicBool,
}

impl NoteState {
    pub const fn new() -> Self {
        // `AtomicU8` is not Copy, so build the arrays with a const block.
        NoteState {
            held: [const { AtomicU8::new(0) }; 128],
            struck: [const { AtomicU8::new(0) }; 128],
            hint: [const { AtomicU8::new(0) }; 128],
            hint_struck: [const { AtomicBool::new(false) }; 128],
            presses: [const { AtomicU8::new(0) }; 128],
            practice: AtomicBool::new(false),
            playing: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn note_on(&self, note: u8, velocity: u8) {
        let n = (note & 0x7f) as usize;
        let v = velocity.max(1);
        self.held[n].store(v, Relaxed);
        self.struck[n].store(v, Relaxed);
        self.presses[n].fetch_add(1, Relaxed);
    }

    #[inline]
    pub fn note_off(&self, note: u8) {
        self.held[(note & 0x7f) as usize].store(0, Relaxed);
    }

    /// Releases every key, e.g. on "All Notes Off" or a device disconnect.
    pub fn all_off(&self) {
        for h in &self.held {
            h.store(0, Relaxed);
        }
    }

    /// Number of times `note` was pressed (wrapping). Lets the player tell a
    /// fresh press from a key that was simply never released.
    #[inline(always)]
    pub fn presses(&self, note: usize) -> u8 {
        self.presses[note].load(Relaxed)
    }

    #[inline(always)]
    pub fn held(&self, note: usize) -> u8 {
        self.held[note].load(Relaxed)
    }

    /// Player sounded `note` (again): lets the renderer show the re-attack.
    pub fn strike_hint(&self, note: u8) {
        self.hint_struck[(note & 0x7f) as usize].store(true, Relaxed);
    }

    #[inline(always)]
    pub fn take_hint_struck(&self, note: usize) -> bool {
        self.hint_struck[note].load(Relaxed) && self.hint_struck[note].swap(false, Relaxed)
    }

    /// Returns and clears the latched strike velocity for `note`.
    #[inline(always)]
    pub fn take_struck(&self, note: usize) -> u8 {
        // Cheap load first: avoids a locked RMW on the 99% of notes that are idle.
        if self.struck[note].load(Relaxed) == 0 {
            return 0;
        }
        self.struck[note].swap(0, Relaxed)
    }

    #[inline(always)]
    pub fn hint(&self, note: usize) -> u8 {
        self.hint[note].load(Relaxed)
    }

    /// A song is playing (keeps the backlight awake).
    pub fn set_playing(&self, on: bool) {
        self.playing.store(on, Relaxed);
    }

    #[inline(always)]
    pub fn playing(&self) -> bool {
        self.playing.load(Relaxed)
    }

    pub fn set_practice(&self, on: bool) {
        self.practice.store(on, Relaxed);
    }

    #[inline(always)]
    pub fn practice(&self) -> bool {
        self.practice.load(Relaxed)
    }

    pub fn set_hint(&self, note: usize, value: u8) {
        self.hint[note].store(value, Relaxed);
    }
}

impl Default for NoteState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strike_latches_until_taken() {
        let s = NoteState::new();
        s.note_on(60, 100);
        s.note_off(60);
        assert_eq!(s.held(60), 0);
        assert_eq!(s.take_struck(60), 100);
        assert_eq!(s.take_struck(60), 0);
    }
}
