//! Pitch trainer: the instrument plays a note, you find it on the keyboard.
//!
//! The note's key is hidden ([`HINT_HIDDEN`]) but still counts as expected,
//! so the renderer shows a right answer green and a wrong one red. After a
//! wrong answer the key is revealed in the right-hand colour until you play
//! it; only the first answer counts towards the score. Runs on the player
//! thread (see `player::run`), which owns MIDI out and the LED hints.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::notes::{HINT_HIDDEN, HINT_RIGHT};

/// How long the instrument holds the note.
const NOTE_MS: f64 = 900.0;
/// Pause after a right answer (or starting) before the next note.
const NEXT_MS: f64 = 1200.0;
const VELOCITY: u8 = 80;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub active: bool,
    /// Lowest and highest note that can be asked (MIDI numbers).
    pub lo: u8,
    pub hi: u8,
    /// The note to find, once answered right or revealed; hidden otherwise.
    pub shown: Option<u8>,
    /// The first answer to this note was wrong.
    pub missed: bool,
    pub streak: u32,
    pub best: u32,
    pub right: u32,
    pub total: u32,
}

/// Partial update from the UI; absent fields are left alone.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Command {
    pub active: Option<bool>,
    pub lo: Option<u8>,
    pub hi: Option<u8>,
    /// Play the current note again.
    pub replay: bool,
    /// Zero the score.
    pub reset: bool,
}

struct State {
    active: bool,
    lo: u8,
    hi: u8,
    target: Option<u8>,
    /// The first answer to the current note is in (it counts, later ones don't).
    answered: bool,
    /// Answered wrong: the key is shown until you play it.
    revealed: bool,
    /// Right answer given; the next note follows after a pause.
    solved: bool,
    /// Until the instrument releases the note; 0 when silent.
    sounding_ms: f64,
    /// Until the next note is asked.
    next_ms: Option<f64>,
    /// Re-read the press counters on the next tick, ignoring earlier presses.
    sync: bool,
    seen: [u8; 128],
    rng: u64,
    streak: u32,
    best: u32,
    right: u32,
    total: u32,
}

pub struct Trainer {
    state: Mutex<State>,
}

impl Trainer {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Trainer {
            state: Mutex::new(State {
                active: false,
                lo: 60, // C4..B4: the octave from middle C
                hi: 71,
                target: None,
                answered: false,
                revealed: false,
                solved: false,
                sounding_ms: 0.0,
                next_ms: None,
                sync: false,
                seen: [0; 128],
                rng: seed | 1,
                streak: 0,
                best: 0,
                right: 0,
                total: 0,
            }),
        }
    }

    pub fn command(&self, c: Command) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        let (lo, hi) = (c.lo.unwrap_or(s.lo), c.hi.unwrap_or(s.hi));
        if !(21..=108).contains(&lo) || !(21..=108).contains(&hi) || hi < lo + 1 {
            return Err("range must be two different notes between A0 (21) and C8 (108)".into());
        }
        if (lo, hi) != (s.lo, s.hi) {
            (s.lo, s.hi) = (lo, hi);
            s.next_note_soon(); // the current note may be outside the new range
        }
        if c.reset {
            (s.streak, s.best, s.right, s.total) = (0, 0, 0, 0);
        }
        if let Some(a) = c.active {
            if a && !s.active {
                s.sync = true;
                s.next_note_soon();
            }
            s.active = a;
        }
        if c.replay && s.active && s.target.is_some() {
            s.sounding_ms = -1.0; // replayed on the next tick
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        Status {
            active: s.active,
            lo: s.lo,
            hi: s.hi,
            // Skipping a note (new range) is not answering it: keep it hidden.
            shown: s.target.filter(|_| s.active && (s.revealed || s.solved && s.answered)),
            missed: s.active && s.revealed,
            streak: s.streak,
            best: s.best,
            right: s.right,
            total: s.total,
        }
    }

    /// Advances by `dt` ms. `presses(k)` is key `k`'s wrapping press counter.
    /// Returns the LED hints while active, `None` when the trainer is off.
    pub fn tick(&self, dt: f64, presses: impl Fn(u8) -> u8, out: &mut impl FnMut([u8; 3])) -> Option<[u8; 128]> {
        let mut s = self.state.lock().unwrap();
        if !s.active {
            if let (Some(t), true) = (s.target.take(), s.sounding_ms != 0.0) {
                out([0x80, t, 0]);
            }
            s.sounding_ms = 0.0;
            return None;
        }
        for k in 0..128u8 {
            let p = presses(k);
            if p != s.seen[k as usize] {
                s.seen[k as usize] = p;
                if !s.sync {
                    s.answer(k);
                }
            }
        }
        s.sync = false;
        if let Some(t) = s.next_ms {
            if t <= dt {
                s.ask(out);
            } else {
                s.next_ms = Some(t - dt);
            }
        }
        if let Some(t) = s.target {
            if s.sounding_ms < 0.0 {
                out([0x80, t, 0]);
                out([0x90, t, VELOCITY]);
                s.sounding_ms = NOTE_MS;
            } else if s.sounding_ms > 0.0 {
                s.sounding_ms -= dt;
                if s.sounding_ms <= 0.0 {
                    out([0x80, t, 0]);
                    s.sounding_ms = 0.0;
                }
            }
        }
        let mut h = [0u8; 128];
        if let Some(t) = s.target.filter(|_| !s.solved) {
            h[t as usize] = HINT_RIGHT | if s.revealed { 0 } else { HINT_HIDDEN };
        }
        Some(h)
    }
}

impl State {
    fn next_note_soon(&mut self) {
        self.solved = true;
        self.next_ms = Some(NEXT_MS);
    }

    fn answer(&mut self, k: u8) {
        let Some(t) = self.target.filter(|_| !self.solved) else { return };
        let first = !self.answered;
        self.answered = true;
        if k == t {
            if first {
                self.right += 1;
                self.total += 1;
                self.streak += 1;
                self.best = self.best.max(self.streak);
            }
            self.next_note_soon();
        } else {
            if first {
                self.total += 1;
                self.streak = 0;
            }
            self.revealed = true;
        }
    }

    fn ask(&mut self, out: &mut impl FnMut([u8; 3])) {
        if let (Some(t), true) = (self.target, self.sounding_ms > 0.0) {
            out([0x80, t, 0]);
        }
        let n = (self.hi - self.lo + 1) as u64;
        let mut k = self.lo + (self.random() % n) as u8;
        if Some(k) == self.target {
            // Never the same note twice in a row: that would be a giveaway.
            k = self.lo + ((k - self.lo) as u64 + 1 + self.random() % (n - 1)) as u8 % n as u8;
        }
        self.target = Some(k);
        (self.answered, self.revealed, self.solved) = (false, false, false);
        self.next_ms = None;
        self.sounding_ms = -1.0;
    }

    /// xorshift64: plenty for picking notes, no dependency.
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks `ms` in 10 ms steps with the given per-key press counters.
    fn run(t: &Trainer, ms: u32, presses: &[u8; 128], out: &mut Vec<[u8; 3]>) -> Option<[u8; 128]> {
        let mut h = None;
        for _ in 0..ms / 10 {
            h = t.tick(10.0, |k| presses[k as usize], &mut |m| out.push(m));
        }
        h
    }

    fn target(t: &Trainer) -> u8 {
        t.state.lock().unwrap().target.unwrap()
    }

    #[test]
    fn plays_a_hidden_note_and_scores_the_first_answer() {
        let t = Trainer::new();
        let mut out = vec![];
        let mut p = [0u8; 128];
        p[30] = 5; // presses from before the trainer started don't count
        t.command(Command { active: Some(true), ..Default::default() }).unwrap();
        let h = run(&t, 1300, &p, &mut out).unwrap();
        let k = target(&t);
        assert!((60..=71).contains(&k));
        assert_eq!(out.last(), Some(&[0x90, k, VELOCITY]), "the note is played");
        assert_eq!(h[k as usize], HINT_RIGHT | HINT_HIDDEN, "expected but not shown");
        assert_eq!(t.status().shown, None);
        assert_eq!(t.status().total, 0);

        // Wrong first: counted, and the key is revealed.
        let wrong = if k == 60 { 61 } else { 60 };
        p[wrong as usize] += 1;
        let h = run(&t, 20, &p, &mut out).unwrap();
        assert_eq!(h[k as usize], HINT_RIGHT);
        let st = t.status();
        assert_eq!((st.right, st.total, st.streak, st.shown, st.missed), (0, 1, 0, Some(k), true));
        // Then right: not counted again, the next note follows.
        p[k as usize] += 1;
        let h = run(&t, 20, &p, &mut out).unwrap();
        assert_eq!(h[k as usize], 0);
        assert_eq!((t.status().right, t.status().total), (0, 1));
        run(&t, 1300, &p, &mut out);
        let k2 = target(&t);
        assert_ne!(k2, k, "never the same note twice in a row");
        p[k2 as usize] += 1;
        run(&t, 20, &p, &mut out);
        let st = t.status();
        assert_eq!((st.right, st.total, st.streak, st.best), (1, 2, 1, 1));
    }

    #[test]
    fn releases_the_note_and_replays_on_request() {
        let t = Trainer::new();
        let mut out = vec![];
        t.command(Command { active: Some(true), ..Default::default() }).unwrap();
        let p = [0u8; 128];
        run(&t, 1300 + NOTE_MS as u32, &p, &mut out);
        let k = target(&t);
        assert_eq!(out.last(), Some(&[0x80, k, 0]), "released after NOTE_MS");
        t.command(Command { replay: true, ..Default::default() }).unwrap();
        run(&t, 10, &p, &mut out);
        assert_eq!(out.last(), Some(&[0x90, k, VELOCITY]));
        // Stopping silences it and clears the hints.
        t.command(Command { active: Some(false), ..Default::default() }).unwrap();
        assert_eq!(run(&t, 10, &p, &mut out), None);
        assert_eq!(out.last(), Some(&[0x80, k, 0]));
    }

    #[test]
    fn changing_the_range_skips_the_note_without_giving_it_away() {
        let t = Trainer::new();
        let mut out = vec![];
        t.command(Command { active: Some(true), ..Default::default() }).unwrap();
        run(&t, 1300, &[0; 128], &mut out);
        t.command(Command { lo: Some(48), hi: Some(59), ..Default::default() }).unwrap();
        assert_eq!(t.status().shown, None);
        run(&t, 1300, &[0; 128], &mut out);
        assert!((48..=59).contains(&target(&t)));
        assert_eq!(t.status().total, 0);
    }

    #[test]
    fn notes_stay_in_range_and_bad_ranges_are_refused() {
        let t = Trainer::new();
        t.command(Command { active: Some(true), lo: Some(48), hi: Some(49), ..Default::default() }).unwrap();
        let mut out = vec![];
        let mut p = [0u8; 128];
        for _ in 0..20 {
            run(&t, 1300, &p, &mut out);
            let k = target(&t);
            assert!(k == 48 || k == 49);
            p[k as usize] += 1;
            run(&t, 10, &p, &mut out);
        }
        assert!(t.command(Command { lo: Some(60), hi: Some(60), ..Default::default() }).is_err());
        assert!(t.command(Command { lo: Some(10), ..Default::default() }).is_err());
    }
}
