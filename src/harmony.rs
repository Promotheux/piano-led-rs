//! Chord progression trainer: the app names a progression in a key (or plays
//! it first, "by ear"), and you play its chords in any voicing or inversion,
//! anywhere on the keyboard. It scores the harmony, not the notes: the root
//! and the chord's quality must match, so a plain V counts for a V7.
//!
//! Runs on the player thread (see `player::run`), like the pitch trainer.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::improv::{detect, Progression, CHORDS};
use crate::notes::HINT_LEFT;

const VELOCITY: u8 = 70;
/// How long each chord sounds when the Pi plays the progression.
const PLAY_MS: f64 = 1200.0;
/// A wrong chord held this long counts as a miss (shorter is a chord still
/// landing, or passing through on the way to the right one).
const WRONG_MS: f64 = 500.0;
/// Pause after a finished progression before the next one.
const NEXT_MS: f64 = 1500.0;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Prompt {
    /// Shown as Roman numerals.
    Symbol,
    /// Played on the piano; the numerals appear as you get them.
    Sound,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Open,
    Right,
    Missed,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub active: bool,
    pub prompt: Prompt,
    pub progression: Progression,
    /// Key as a pitch class, C = 0.
    pub key: u8,
    pub random_key: bool,
    /// The chords to play as `[semitones above the key, shape]`.
    pub steps: Vec<(u8, u8)>,
    pub marks: Vec<Mark>,
    pub step: usize,
    /// The Pi is playing the progression (no judging meanwhile).
    pub playing: bool,
    /// The chord you're holding, as `[root pitch class, shape]`.
    pub heard: Option<(u8, u8)>,
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
    pub prompt: Option<Prompt>,
    pub progression: Option<Progression>,
    pub key: Option<u8>,
    pub random_key: Option<bool>,
    /// Play the progression again.
    pub replay: Option<bool>,
    /// Zero the score.
    pub reset: Option<bool>,
}

struct State {
    active: bool,
    prompt: Prompt,
    progression: Progression,
    key: u8,
    random_key: bool,
    steps: Vec<(u8, u8)>,
    marks: Vec<Mark>,
    step: usize,
    heard: Option<(u8, u8)>,
    /// The chord just accepted: not judged again until you change it.
    accepted: Option<(u8, u8)>,
    wrong_ms: f64,
    /// Playback position in ms, while the Pi plays the progression.
    play_ms: Option<f64>,
    sounding: [u8; 4],
    n_sounding: usize,
    /// Countdown to the next progression once this one is done.
    next_ms: Option<f64>,
    /// Start a new progression on the next tick.
    fresh: bool,
    streak: u32,
    best: u32,
    right: u32,
    total: u32,
    rng: u64,
}

pub struct Harmony {
    state: Mutex<State>,
}

/// The triad a chord is built on: 7ths, 9ths etc. don't change its function.
fn quality(shape: u8) -> u8 {
    match shape {
        4 | 5 => 0, // 7, maj7 → major
        6 => 1,     // m7 → minor
        7 => 2,     // m7♭5 → diminished
        s => s,
    }
}

/// The progression's chords, with repeats of the same chord merged (the
/// 12-bar blues holds I7 for four bars: you play it once).
fn steps_of(p: Progression) -> Vec<(u8, u8)> {
    let mut v: Vec<(u8, u8)> = p.bars().to_vec();
    v.dedup();
    v
}

impl Harmony {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Harmony {
            state: Mutex::new(State {
                active: false,
                prompt: Prompt::Symbol,
                progression: Progression::Basic,
                key: 0,
                random_key: false,
                steps: Vec::new(),
                marks: Vec::new(),
                step: 0,
                heard: None,
                accepted: None,
                wrong_ms: 0.0,
                play_ms: None,
                sounding: [0; 4],
                n_sounding: 0,
                next_ms: None,
                fresh: false,
                streak: 0,
                best: 0,
                right: 0,
                total: 0,
                rng: seed | 1,
            }),
        }
    }

    pub fn command(&self, c: Command) -> Result<(), String> {
        if c.key.is_some_and(|k| k > 11) {
            return Err("key must be a pitch class 0 (C) to 11 (B)".into());
        }
        let mut s = self.state.lock().unwrap();
        let restart = c.active == Some(true) && !s.active
            || c.prompt.is_some_and(|x| x != s.prompt)
            || c.progression.is_some_and(|x| x != s.progression)
            || c.key.is_some_and(|x| x != s.key)
            || c.random_key.is_some_and(|x| x != s.random_key);
        if let Some(x) = c.prompt {
            s.prompt = x;
        }
        if let Some(x) = c.progression {
            s.progression = x;
        }
        if let Some(x) = c.key {
            s.key = x;
        }
        if let Some(x) = c.random_key {
            s.random_key = x;
        }
        if let Some(x) = c.active {
            s.active = x;
        }
        if c.reset == Some(true) {
            (s.streak, s.best, s.right, s.total) = (0, 0, 0, 0);
        }
        if restart {
            s.fresh = true;
        }
        if c.replay == Some(true) && s.active {
            s.play_ms = Some(0.0);
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        Status {
            active: s.active,
            prompt: s.prompt,
            progression: s.progression,
            key: s.key,
            random_key: s.random_key,
            steps: s.steps.clone(),
            marks: s.marks.clone(),
            step: s.step,
            playing: s.play_ms.is_some(),
            heard: s.heard.filter(|_| s.active),
            streak: s.streak,
            best: s.best,
            right: s.right,
            total: s.total,
        }
    }

    /// Advances by `dt` ms. `held(k)` says whether key `k` is down. Returns
    /// the LED hints while active, `None` when the trainer is off.
    pub fn tick(&self, dt: f64, held: impl Fn(u8) -> bool, out: &mut impl FnMut([u8; 3])) -> Option<[u8; 128]> {
        let mut s = self.state.lock().unwrap();
        if !s.active {
            s.release(out);
            s.play_ms = None;
            return None;
        }
        if s.fresh {
            s.new_round(out);
        }
        if let Some(t) = s.play_ms {
            s.playback(t, dt, out);
            return Some([0; 128]);
        }
        if let Some(t) = s.next_ms {
            if t <= dt {
                s.new_round(out);
            } else {
                s.next_ms = Some(t - dt);
            }
        }

        let mut keys = [0u8; 16];
        let mut n = 0;
        for k in (21..=108).filter(|&k| held(k)) {
            if n < keys.len() {
                keys[n] = k;
                n += 1;
            }
        }
        // Three keys make a chord; fewer is a chord still landing (or let go).
        s.heard = if n >= 3 { detect(&keys[..n]) } else { None };
        s.judge(dt);
        Some(s.hints())
    }
}

impl State {
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn new_round(&mut self, out: &mut impl FnMut([u8; 3])) {
        self.fresh = false;
        if self.random_key {
            self.key = (self.random() % 12) as u8;
        }
        self.steps = steps_of(self.progression);
        self.marks = vec![Mark::Open; self.steps.len()];
        (self.step, self.wrong_ms, self.next_ms) = (0, 0.0, None);
        // Whatever you hold now belongs to the last round.
        self.accepted = self.heard;
        self.release(out);
        self.play_ms = (self.prompt == Prompt::Sound).then_some(0.0);
    }

    /// The step's chord in the current key, as `(root pitch class, shape)`.
    fn target(&self, i: usize) -> (u8, u8) {
        let (degree, shape) = self.steps[i];
        ((self.key + degree) % 12, shape)
    }

    /// Plays the progression, one chord per [`PLAY_MS`].
    fn playback(&mut self, t: f64, dt: f64, out: &mut impl FnMut([u8; 3])) {
        let (from, to) = ((t / PLAY_MS) as usize, ((t + dt) / PLAY_MS) as usize);
        if t == 0.0 || to != from {
            self.release(out);
            match self.steps.get(if t == 0.0 { 0 } else { to }).copied() {
                Some(_) => {
                    let chord = self.target(if t == 0.0 { 0 } else { to });
                    self.strike(chord, out);
                }
                None => {
                    self.play_ms = None;
                    return;
                }
            }
        }
        self.play_ms = Some(t + dt);
    }

    /// Strikes `chord` around middle C (root C3..F3, a close voicing above).
    fn strike(&mut self, (root, shape): (u8, u8), out: &mut impl FnMut([u8; 3])) {
        let base = if root > 5 { 36 + root } else { 48 + root };
        for &i in CHORDS[shape as usize] {
            out([0x90, base + i, VELOCITY]);
            self.sounding[self.n_sounding] = base + i;
            self.n_sounding += 1;
        }
    }

    fn release(&mut self, out: &mut impl FnMut([u8; 3])) {
        for &k in &self.sounding[..self.n_sounding] {
            out([0x80, k, 0]);
        }
        self.n_sounding = 0;
    }

    fn judge(&mut self, dt: f64) {
        if self.step >= self.steps.len() {
            return;
        }
        // Still holding the chord just accepted: wait until it changes.
        if self.accepted.is_some() && self.heard == self.accepted {
            return;
        }
        self.accepted = None;
        let Some((root, shape)) = self.heard else {
            self.wrong_ms = 0.0;
            return;
        };
        let (want_root, want_shape) = self.target(self.step);
        if root == want_root && quality(shape) == quality(want_shape) {
            let first_try = self.marks[self.step] == Mark::Open;
            self.marks[self.step] = if first_try { Mark::Right } else { Mark::Missed };
            self.total += 1;
            if first_try {
                self.right += 1;
                self.streak += 1;
                self.best = self.best.max(self.streak);
            }
            self.step += 1;
            self.wrong_ms = 0.0;
            self.accepted = self.heard;
            if self.step == self.steps.len() {
                self.next_ms = Some(NEXT_MS);
            }
        } else {
            self.wrong_ms += dt;
            if self.wrong_ms >= WRONG_MS && self.marks[self.step] == Mark::Open {
                self.marks[self.step] = Mark::Missed;
                self.streak = 0;
            }
        }
    }

    /// Blind until you miss: then the chord to play lights up (close voicing
    /// around middle C), so you can find it and carry on.
    fn hints(&self) -> [u8; 128] {
        let mut h = [0u8; 128];
        if self.marks.get(self.step) == Some(&Mark::Missed) {
            let (root, shape) = self.target(self.step);
            for &i in CHORDS[shape as usize] {
                h[(48 + root + i) as usize] = HINT_LEFT;
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C3: u8 = 48;

    fn start(p: Progression, prompt: Prompt) -> Harmony {
        let h = Harmony::new();
        let c = Command { active: Some(true), progression: Some(p), prompt: Some(prompt), key: Some(0), ..Default::default() };
        h.command(c).unwrap();
        h
    }

    /// Holds `keys` for `ms`, ticking in 10 ms steps; returns the last hints.
    fn hold(h: &Harmony, keys: &[u8], ms: u32) -> Option<[u8; 128]> {
        let mut out = None;
        for _ in 0..ms / 10 {
            out = h.tick(10.0, |k| keys.contains(&k), &mut |_| {});
        }
        out
    }

    #[test]
    fn any_voicing_of_the_right_chords_scores() {
        let h = start(Progression::Basic, Prompt::Symbol); // I–IV–V–I in C
        hold(&h, &[C3 + 4, C3 + 7, C3 + 12], 50); // C major, first inversion
        hold(&h, &[], 50);
        hold(&h, &[C3 - 7, C3 + 9, C3 + 12, C3 + 17], 50); // F major, spread
        hold(&h, &[], 50);
        hold(&h, &[C3 + 7, C3 + 11, C3 + 14, C3 + 17], 50); // G7 counts for V
        hold(&h, &[], 50);
        hold(&h, &[C3, C3 + 4, C3 + 7], 50);
        let s = h.status();
        assert_eq!(s.marks, vec![Mark::Right; 4]);
        assert_eq!((s.right, s.total, s.streak), (4, 4, 4));
    }

    #[test]
    fn holding_the_last_chord_does_not_count_for_the_next() {
        // I–IV–V–I: the final I must be played again, not carried over.
        let h = start(Progression::Basic, Prompt::Symbol);
        hold(&h, &[C3, C3 + 4, C3 + 7], 50);
        hold(&h, &[C3, C3 + 4, C3 + 7], 1000); // still held: no miss for IV
        assert_eq!(h.status().marks[1], Mark::Open);
    }

    #[test]
    fn a_wrong_chord_held_is_a_miss_and_shows_the_answer() {
        let h = start(Progression::Basic, Prompt::Symbol);
        let hints = hold(&h, &[C3 + 2, C3 + 5, C3 + 9], 600).unwrap(); // Dm instead of C
        let s = h.status();
        assert_eq!((s.marks[0], s.streak), (Mark::Missed, 0));
        assert_eq!((hints[48], hints[52], hints[55]), (HINT_LEFT, HINT_LEFT, HINT_LEFT));
        // Playing it right afterwards moves on but doesn't score.
        hold(&h, &[C3, C3 + 4, C3 + 7], 50);
        let s = h.status();
        assert_eq!((s.step, s.right, s.total), (1, 0, 1));
        assert_eq!(s.marks[0], Mark::Missed);
    }

    #[test]
    fn by_ear_plays_the_progression_first() {
        let h = start(Progression::Basic, Prompt::Sound);
        let mut on = Vec::new();
        for _ in 0..600 {
            h.tick(10.0, |_| false, &mut |m| {
                if m[0] == 0x90 {
                    on.push(m[1]);
                }
            });
        }
        // C, F, G, C: three notes each, then it waits for you.
        assert_eq!(on.len(), 12);
        assert_eq!(&on[..3], &[48, 52, 55]);
        assert!(!h.status().playing);
    }

    #[test]
    fn repeated_bars_are_one_step() {
        assert_eq!(steps_of(Progression::Blues).len(), 7);
        assert_eq!(steps_of(Progression::Jazz).len(), 3);
    }
}
