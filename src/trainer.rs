//! Pitch trainer: the instrument plays a note, an interval or a chord, and
//! you play it back on the keyboard.
//!
//! The keys to find are hidden ([`HINT_HIDDEN`]) but still count as expected,
//! so the renderer shows right keys green and wrong ones red. A wrong key
//! reveals the missing ones in the right-hand colour until you play them;
//! only a question answered without a wrong key counts as right. Runs on the
//! player thread (see `player::run`), which owns MIDI out and the LED hints.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::notes::{HINT_HIDDEN, HINT_RIGHT};

/// Single note: how long the instrument holds it.
const NOTE_MS: f64 = 900.0;
/// Interval: each of its two notes, one after the other.
const STEP_MS: f64 = 700.0;
/// Chord: all notes together.
const CHORD_MS: f64 = 1300.0;
/// Pause after a right answer (or starting) before the next question.
const NEXT_MS: f64 = 1200.0;
const VELOCITY: u8 = 80;
/// Triads in root position, as semitones above the root: major, minor,
/// diminished, augmented (the UI names them in this order).
const CHORDS: [[u8; 3]; 4] = [[0, 4, 7], [0, 3, 7], [0, 3, 6], [0, 4, 8]];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Drill {
    Note,
    Interval,
    Chord,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub active: bool,
    pub drill: Drill,
    /// Lowest and highest note that can be asked (MIDI numbers).
    pub lo: u8,
    pub hi: u8,
    /// The keys of the question, low to high, once answered or revealed;
    /// empty while you're still listening.
    pub shown: Vec<u8>,
    /// Interval: semitones; chord: index into the major/minor/dim/aug list.
    pub kind: u8,
    /// A wrong key was played for this question.
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
    /// Switching drills starts a fresh score.
    pub drill: Option<Drill>,
    pub lo: Option<u8>,
    pub hi: Option<u8>,
    /// Play the current question again.
    pub replay: bool,
    /// Zero the score.
    pub reset: bool,
}

struct State {
    active: bool,
    drill: Drill,
    lo: u8,
    hi: u8,
    /// Keys of the current question, low to high; `len` 0 = none yet.
    keys: [u8; 3],
    len: usize,
    kind: u8,
    /// Keys of the question already played.
    hit: [bool; 3],
    /// A wrong key was played: score it and show the missing keys.
    missed: bool,
    /// Every key played; the next question follows after a pause.
    solved: bool,
    /// Time since the instrument started playing the question; `None` once
    /// it's done, negative to (re)start on the next tick.
    play_ms: Option<f64>,
    /// Until the next question is asked.
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
                drill: Drill::Note,
                lo: 60, // C4..B4: the octave from middle C
                hi: 71,
                keys: [0; 3],
                len: 0,
                kind: 0,
                hit: [false; 3],
                missed: false,
                solved: false,
                play_ms: None,
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
        if c.drill.unwrap_or(s.drill) == Drill::Chord && hi - lo < 8 {
            return Err("chords need a range of at least 8 semitones".into());
        }
        if (lo, hi) != (s.lo, s.hi) {
            (s.lo, s.hi) = (lo, hi);
            s.skip(); // the current question may be outside the new range
        }
        if let Some(d) = c.drill.filter(|&d| d != s.drill) {
            s.drill = d;
            s.reset_score();
            s.skip();
        }
        if c.reset {
            s.reset_score();
        }
        if let Some(a) = c.active {
            if a && !s.active {
                s.sync = true;
                s.skip();
            }
            s.active = a;
        }
        if c.replay && s.active && s.len > 0 {
            s.play_ms = Some(-1.0);
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        // Skipping a question (new range or drill) is not answering it.
        let shown = s.active && s.len > 0 && (s.missed || s.solved && s.hit[0]);
        Status {
            active: s.active,
            drill: s.drill,
            lo: s.lo,
            hi: s.hi,
            shown: if shown { s.keys[..s.len].to_vec() } else { Vec::new() },
            kind: s.kind,
            missed: s.active && s.missed,
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
            if s.play_ms.take().is_some() {
                s.silence(out);
            }
            s.len = 0;
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
        s.play(dt, out);
        let mut h = [0u8; 128];
        if !s.solved {
            for i in (0..s.len).filter(|&i| !s.hit[i]) {
                h[s.keys[i] as usize] = HINT_RIGHT | if s.missed { 0 } else { HINT_HIDDEN };
            }
        }
        Some(h)
    }
}

impl State {
    fn reset_score(&mut self) {
        (self.streak, self.best, self.right, self.total) = (0, 0, 0, 0);
    }

    /// Moves on to a new question after a pause, without scoring this one.
    fn skip(&mut self) {
        self.solved = true;
        self.hit = [false; 3];
        self.next_ms = Some(NEXT_MS);
    }

    fn answer(&mut self, k: u8) {
        if self.len == 0 || self.solved {
            return;
        }
        match self.keys[..self.len].iter().position(|&q| q == k) {
            Some(i) => {
                self.hit[i] = true;
                if self.hit[..self.len].iter().all(|&h| h) {
                    if !self.missed {
                        self.right += 1;
                        self.total += 1;
                        self.streak += 1;
                        self.best = self.best.max(self.streak);
                    }
                    self.solved = true;
                    self.next_ms = Some(NEXT_MS);
                }
            }
            None if !self.missed => {
                self.missed = true;
                self.total += 1;
                self.streak = 0;
            }
            None => {}
        }
    }

    fn ask(&mut self, out: &mut impl FnMut([u8; 3])) {
        if self.play_ms.is_some() {
            self.silence(out); // the last question is still sounding
        }
        // A question you just had would be a giveaway; a few tries is plenty.
        let last = (self.keys, self.len);
        for _ in 0..8 {
            self.pick();
            if (self.keys, self.len) != last {
                break;
            }
        }
        (self.hit, self.missed, self.solved) = ([false; 3], false, false);
        self.next_ms = None;
        self.play_ms = Some(0.0);
    }

    /// Draws a question that fits in `lo..=hi`.
    fn pick(&mut self) {
        let span = self.hi - self.lo;
        let shape: [u8; 3];
        (shape, self.len) = match self.drill {
            Drill::Note => ([0; 3], 1),
            Drill::Interval => {
                let semis = 1 + (self.random() % span.min(12) as u64) as u8;
                self.kind = semis;
                ([0, semis, 0], 2)
            }
            Drill::Chord => {
                // Every triad fits: `command` asks for at least 8 semitones.
                let i = (self.random() % CHORDS.len() as u64) as usize;
                self.kind = i as u8;
                (CHORDS[i], 3)
            }
        };
        let top = shape[self.len - 1];
        let root = self.lo + (self.random() % (span - top + 1) as u64) as u8;
        for i in 0..self.len {
            self.keys[i] = root + shape[i];
        }
    }

    /// Plays the question: one note, two in turn, or all at once. Each tick
    /// sends the events that fall in `[t, t + dt)`.
    fn play(&mut self, dt: f64, out: &mut impl FnMut([u8; 3])) {
        let Some(mut t) = self.play_ms else { return };
        if t < 0.0 {
            self.silence(out); // a replay cuts the previous playback short
            t = 0.0;
        }
        let (len, gap, hold) = match self.drill {
            Drill::Note => (1, 0.0, NOTE_MS),
            Drill::Interval => (2, STEP_MS, STEP_MS),
            Drill::Chord => (3, 0.0, CHORD_MS),
        };
        let now = |x: f64| t <= x && x < t + dt;
        for (i, &k) in self.keys[..len].iter().enumerate() {
            let on = i as f64 * gap;
            if now(on) {
                out([0x90, k, VELOCITY]);
            }
            if now(on + hold) {
                out([0x80, k, 0]);
            }
        }
        let end = (len - 1) as f64 * gap + hold;
        self.play_ms = if t + dt > end { None } else { Some(t + dt) };
    }

    fn silence(&self, out: &mut impl FnMut([u8; 3])) {
        for &k in &self.keys[..self.len] {
            out([0x80, k, 0]);
        }
    }

    /// xorshift64: plenty for picking questions, no dependency.
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

    fn keys(t: &Trainer) -> Vec<u8> {
        let s = t.state.lock().unwrap();
        s.keys[..s.len].to_vec()
    }

    fn start(drill: Drill) -> Trainer {
        let t = Trainer::new();
        t.command(Command { active: Some(true), drill: Some(drill), ..Default::default() }).unwrap();
        t
    }

    #[test]
    fn plays_a_hidden_note_and_scores_the_first_answer() {
        let t = Trainer::new();
        let mut out = vec![];
        let mut p = [0u8; 128];
        p[30] = 5; // presses from before the trainer started don't count
        t.command(Command { active: Some(true), ..Default::default() }).unwrap();
        let h = run(&t, 1300, &p, &mut out).unwrap();
        let k = keys(&t)[0];
        assert!((60..=71).contains(&k));
        assert_eq!(out, vec![[0x90, k, VELOCITY]], "the note is played");
        assert_eq!(h[k as usize], HINT_RIGHT | HINT_HIDDEN, "expected but not shown");
        assert!(t.status().shown.is_empty());
        assert_eq!(t.status().total, 0);

        // Wrong first: counted, and the key is revealed.
        let wrong = if k == 60 { 61 } else { 60 };
        p[wrong as usize] += 1;
        let h = run(&t, 20, &p, &mut out).unwrap();
        assert_eq!(h[k as usize], HINT_RIGHT);
        let st = t.status();
        assert_eq!((st.right, st.total, st.streak, st.shown, st.missed), (0, 1, 0, vec![k], true));
        // Then right: not counted again, the next note follows.
        p[k as usize] += 1;
        let h = run(&t, 20, &p, &mut out).unwrap();
        assert_eq!(h[k as usize], 0);
        assert_eq!((t.status().right, t.status().total), (0, 1));
        run(&t, 1300, &p, &mut out);
        let k2 = keys(&t)[0];
        assert_ne!(k2, k, "never the same note twice in a row");
        p[k2 as usize] += 1;
        run(&t, 20, &p, &mut out);
        let st = t.status();
        assert_eq!((st.right, st.total, st.streak, st.best), (1, 2, 1, 1));
    }

    #[test]
    fn releases_the_note_and_replays_on_request() {
        let t = start(Drill::Note);
        let mut out = vec![];
        let p = [0u8; 128];
        run(&t, 1300 + NOTE_MS as u32, &p, &mut out);
        let k = keys(&t)[0];
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
    fn an_interval_plays_its_notes_in_turn_and_needs_both_back() {
        let t = start(Drill::Interval);
        let mut out = vec![];
        let mut p = [0u8; 128];
        run(&t, 1250, &p, &mut out);
        let k = keys(&t);
        assert!(k[0] < k[1] && k[1] - k[0] <= 11 && k[0] >= 60 && k[1] <= 71, "{k:?} in C4..B4");
        assert_eq!(out, vec![[0x90, k[0], VELOCITY]]);
        run(&t, 1500, &p, &mut out);
        assert_eq!(out, vec![[0x90, k[0], VELOCITY], [0x80, k[0], 0], [0x90, k[1], VELOCITY], [0x80, k[1], 0]]);
        assert_eq!(t.status().kind, k[1] - k[0]);
        // One key isn't enough; both are.
        p[k[0] as usize] += 1;
        run(&t, 20, &p, &mut out);
        assert!(t.status().shown.is_empty() && t.status().total == 0);
        p[k[1] as usize] += 1;
        run(&t, 20, &p, &mut out);
        let st = t.status();
        assert_eq!((st.shown, st.right, st.total), (k, 1, 1));
    }

    #[test]
    fn a_chord_is_played_together_and_a_wrong_key_reveals_the_rest() {
        let t = start(Drill::Chord);
        let mut out = vec![];
        let mut p = [0u8; 128];
        let h = run(&t, 1250, &p, &mut out).unwrap();
        let k = keys(&t);
        assert_eq!(out, k.iter().map(|&k| [0x90, k, VELOCITY]).collect::<Vec<_>>(), "all at once");
        let shape = [k[1] - k[0], k[2] - k[0]];
        assert_eq!(CHORDS[t.status().kind as usize][1..], shape);
        assert!(k.iter().all(|&k| h[k as usize] == HINT_RIGHT | HINT_HIDDEN));
        p[k[0] as usize] += 1; // root right...
        p[k[0] as usize + 1] += 1; // ...then a wrong key
        let h = run(&t, 20, &p, &mut out).unwrap();
        assert_eq!((h[k[0] as usize], h[k[1] as usize], h[k[2] as usize]), (0, HINT_RIGHT, HINT_RIGHT));
        let st = t.status();
        assert_eq!((st.missed, st.total, st.shown), (true, 1, k.clone()));
        p[k[1] as usize] += 1;
        p[k[2] as usize] += 1;
        run(&t, 20, &p, &mut out);
        assert_eq!((t.status().right, t.status().total), (0, 1), "finished, but not right");
    }

    #[test]
    fn changing_the_range_or_drill_skips_without_giving_it_away() {
        let t = start(Drill::Note);
        let mut out = vec![];
        let mut p = [0u8; 128];
        run(&t, 1300, &p, &mut out);
        p[keys(&t)[0] as usize] += 1;
        run(&t, 20, &p, &mut out);
        assert_eq!(t.status().right, 1);
        t.command(Command { lo: Some(48), hi: Some(59), ..Default::default() }).unwrap();
        assert!(t.status().shown.is_empty());
        run(&t, 1300, &p, &mut out);
        assert!((48..=59).contains(&keys(&t)[0]));
        t.command(Command { drill: Some(Drill::Chord), ..Default::default() }).unwrap();
        assert!(t.status().shown.is_empty());
        assert_eq!(t.status().total, 0, "a new drill starts a fresh score");
    }

    #[test]
    fn questions_stay_in_range_and_bad_ranges_are_refused() {
        for drill in [Drill::Note, Drill::Interval, Drill::Chord] {
            let t = start(drill);
            t.command(Command { lo: Some(48), hi: Some(56), ..Default::default() }).unwrap();
            let mut out = vec![];
            let mut p = [0u8; 128];
            for _ in 0..30 {
                run(&t, 1300, &p, &mut out);
                let k = keys(&t);
                assert!(k.iter().all(|&k| (48..=56).contains(&k)), "{drill:?} {k:?}");
                assert!(k.windows(2).all(|w| w[0] < w[1]), "{drill:?} {k:?}");
                for &k in &k {
                    p[k as usize] += 1;
                }
                run(&t, 10, &p, &mut out);
            }
        }
        let t = Trainer::new();
        assert!(t.command(Command { lo: Some(60), hi: Some(60), ..Default::default() }).is_err());
        assert!(t.command(Command { lo: Some(10), ..Default::default() }).is_err());
        let narrow = Command { drill: Some(Drill::Chord), lo: Some(60), hi: Some(67), ..Default::default() };
        assert!(t.command(narrow).is_err(), "an augmented triad wouldn't fit");
    }
}
