//! Improv mode: the LEDs show which notes sound good right now.
//!
//! The chord comes from you (keys held below the split, recognised live and
//! kept after you let go) or from a backing progression the instrument plays.
//! Above the split, the chord's notes light bright in the left-hand colour
//! (safe to land on), the rest of the scale dim in the right-hand colour
//! (passing notes), everything else stays dark. Runs on the player thread
//! (see `player::run`), which owns MIDI out and the LED hints.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::notes::{HINT_LEFT, HINT_PREVIEW, HINT_RIGHT};

/// Keys below this are the chord hand; the LEDs guide the keys from here up.
pub const SPLIT: u8 = 60; // C4
const VELOCITY: u8 = 70;

/// Chord shapes as semitones above the root; the UI names them in this
/// order: major, minor, dim, aug, 7, maj7, m7, m7♭5, sus2, sus4, 5.
const CHORDS: [&[u8]; 11] = [
    &[0, 4, 7],
    &[0, 3, 7],
    &[0, 3, 6],
    &[0, 4, 8],
    &[0, 4, 7, 10],
    &[0, 4, 7, 11],
    &[0, 3, 7, 10],
    &[0, 3, 6, 10],
    &[0, 2, 7],
    &[0, 5, 7],
    &[0, 7],
];
const MAJ: u8 = 0;
const MIN: u8 = 1;
const DOM7: u8 = 4;
const MAJ7: u8 = 5;
const MIN7: u8 = 6;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// You play the chords with your left hand.
    Lead,
    /// The instrument plays a progression.
    Backing,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    Major,
    Minor,
    Blues,
}

impl Scale {
    fn notes(self, pentatonic: bool) -> &'static [u8] {
        match (self, pentatonic) {
            (Scale::Major, false) => &[0, 2, 4, 5, 7, 9, 11],
            (Scale::Major, true) => &[0, 2, 4, 7, 9],
            (Scale::Minor, false) => &[0, 2, 3, 5, 7, 8, 10],
            (Scale::Minor | Scale::Blues, true) => &[0, 3, 5, 7, 10],
            (Scale::Blues, false) => &[0, 3, 5, 6, 7, 10],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Progression {
    /// I–V–vi–IV
    Pop,
    /// I–vi–IV–V
    Fifties,
    /// ii7–V7–Imaj7
    Jazz,
    /// 12-bar blues
    Blues,
    /// i–VI–III–VII
    Minor,
}

impl Progression {
    /// One chord per bar: (semitones above the key, chord shape).
    fn bars(self) -> &'static [(u8, u8)] {
        match self {
            Progression::Pop => &[(0, MAJ), (7, MAJ), (9, MIN), (5, MAJ)],
            Progression::Fifties => &[(0, MAJ), (9, MIN), (5, MAJ), (7, MAJ)],
            Progression::Jazz => &[(2, MIN7), (7, DOM7), (0, MAJ7), (0, MAJ7)],
            Progression::Blues => &[
                (0, DOM7),
                (0, DOM7),
                (0, DOM7),
                (0, DOM7),
                (5, DOM7),
                (5, DOM7),
                (0, DOM7),
                (0, DOM7),
                (7, DOM7),
                (5, DOM7),
                (0, DOM7),
                (7, DOM7),
            ],
            Progression::Minor => &[(0, MIN), (8, MAJ), (3, MAJ), (10, MAJ)],
        }
    }

    fn scale(self) -> Scale {
        match self {
            Progression::Blues => Scale::Blues,
            Progression::Minor => Scale::Minor,
            _ => Scale::Major,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub active: bool,
    pub source: Source,
    /// Key as a pitch class, C = 0.
    pub key: u8,
    pub scale: Scale,
    pub pentatonic: bool,
    pub progression: Progression,
    pub tempo: u16,
    pub split: u8,
    /// Current chord as `[root pitch class, shape]`.
    pub chord: Option<(u8, u8)>,
    /// Backing: the next bar's chord, and where we are in the bar.
    pub next: Option<(u8, u8)>,
    pub beat: u8,
}

/// Partial update from the UI; absent fields are left alone.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Command {
    pub active: Option<bool>,
    pub source: Option<Source>,
    pub key: Option<u8>,
    pub scale: Option<Scale>,
    pub pentatonic: Option<bool>,
    /// Also sets the scale to the progression's.
    pub progression: Option<Progression>,
    pub tempo: Option<u16>,
}

struct State {
    active: bool,
    source: Source,
    key: u8,
    scale: Scale,
    pentatonic: bool,
    progression: Progression,
    tempo: u16,
    chord: Option<(u8, u8)>,
    /// Backing: time since it started, and the keys the instrument holds.
    clock_ms: f64,
    bar: Option<usize>,
    sounding: [u8; 4],
    n_sounding: usize,
}

pub struct Improv {
    state: Mutex<State>,
}

impl Improv {
    pub fn new() -> Self {
        Improv {
            state: Mutex::new(State {
                active: false,
                source: Source::Lead,
                key: 0,
                scale: Scale::Major,
                pentatonic: false,
                progression: Progression::Pop,
                tempo: 80,
                chord: None,
                clock_ms: 0.0,
                bar: None,
                sounding: [0; 4],
                n_sounding: 0,
            }),
        }
    }

    pub fn command(&self, c: Command) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if c.key.is_some_and(|k| k > 11) {
            return Err("key must be a pitch class 0 (C) to 11 (B)".into());
        }
        if c.tempo.is_some_and(|t| !(40..=200).contains(&t)) {
            return Err("tempo must be 40 to 200 bpm".into());
        }
        let restart = c.active == Some(true) && !s.active
            || c.source.is_some_and(|x| x != s.source)
            || c.progression.is_some_and(|x| x != s.progression)
            || c.key.is_some_and(|x| x != s.key);
        if let Some(x) = c.source {
            s.source = x;
        }
        if let Some(x) = c.key {
            s.key = x;
        }
        if let Some(p) = c.progression {
            s.progression = p;
            s.scale = p.scale();
        }
        if let Some(x) = c.scale {
            s.scale = x;
        }
        if let Some(x) = c.pentatonic {
            s.pentatonic = x;
        }
        if let Some(x) = c.tempo {
            s.tempo = x;
        }
        if let Some(a) = c.active {
            s.active = a;
        }
        if restart {
            (s.clock_ms, s.bar, s.chord) = (0.0, None, None);
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        let backing = s.active && s.source == Source::Backing;
        let bars = s.progression.bars();
        Status {
            active: s.active,
            source: s.source,
            key: s.key,
            scale: s.scale,
            pentatonic: s.pentatonic,
            progression: s.progression,
            tempo: s.tempo,
            split: SPLIT,
            chord: s.chord.filter(|_| s.active),
            next: s.bar.filter(|_| backing).map(|b| s.chord_of(bars[(b + 1) % bars.len()])),
            beat: if backing { ((s.clock_ms / s.beat_ms()) as u64 % 4) as u8 } else { 0 },
        }
    }

    /// Advances by `dt` ms. `held(k)` says whether key `k` is down. Returns
    /// the LED hints while active, `None` when improv mode is off.
    pub fn tick(&self, dt: f64, held: impl Fn(u8) -> bool, out: &mut impl FnMut([u8; 3])) -> Option<[u8; 128]> {
        let mut s = self.state.lock().unwrap();
        if !s.active || s.source == Source::Lead {
            s.release(out);
            s.bar = None;
        }
        if !s.active {
            return None;
        }
        match s.source {
            Source::Lead => {
                // The chord stays after you let go, until you play another.
                let mut keys = [0u8; 16];
                let mut n = 0;
                for k in (21..SPLIT).filter(|&k| held(k)) {
                    if n < keys.len() {
                        keys[n] = k;
                        n += 1;
                    }
                }
                if n > 0 {
                    s.chord = detect(&keys[..n]);
                }
            }
            Source::Backing => {
                let bars = s.progression.bars();
                let bar = (s.clock_ms / (4.0 * s.beat_ms())) as usize % bars.len();
                if s.bar != Some(bar) {
                    s.bar = Some(bar);
                    let chord = s.chord_of(bars[bar]);
                    s.chord = Some(chord);
                    s.play(chord, out);
                }
                s.clock_ms += dt;
            }
        }
        Some(s.hints())
    }
}

impl State {
    fn beat_ms(&self) -> f64 {
        60_000.0 / self.tempo as f64
    }

    /// A progression step as an actual chord in the current key.
    fn chord_of(&self, (degree, shape): (u8, u8)) -> (u8, u8) {
        ((self.key + degree) % 12, shape)
    }

    /// Strikes `chord` low in the bass (root C3..F3, a close voicing above).
    fn play(&mut self, (root, shape): (u8, u8), out: &mut impl FnMut([u8; 3])) {
        self.release(out);
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

    fn hints(&self) -> [u8; 128] {
        let scale = self.scale.notes(self.pentatonic);
        let chord = self.chord.map(|(root, shape)| (root, CHORDS[shape as usize]));
        let mut h = [0u8; 128];
        for k in SPLIT..=108 {
            let pc = k % 12;
            if chord.is_some_and(|(root, tones)| tones.contains(&((pc + 12 - root) % 12))) {
                h[k as usize] = HINT_LEFT;
            } else if scale.contains(&((pc + 12 - self.key) % 12)) {
                h[k as usize] = HINT_RIGHT | HINT_PREVIEW;
            }
        }
        h
    }
}

/// Names the chord in `keys` (held keys, low to high) as `(root, shape)`.
/// An exact match wins, then the smallest chord that contains every held
/// note (C and E alone read as C major); ties go to the bass note as root.
pub fn detect(keys: &[u8]) -> Option<(u8, u8)> {
    let mask = |root: u8, shape: &[u8]| shape.iter().fold(0u16, |m, &i| m | 1 << ((root + i) % 12));
    let held = keys.iter().fold(0u16, |m, &k| m | 1 << (k % 12));
    let bass = keys.first()? % 12;
    let mut best: Option<((bool, usize, bool, usize), (u8, u8))> = None;
    for root in 0..12u8 {
        for (i, shape) in CHORDS.iter().enumerate() {
            let m = mask(root, shape);
            if m & held != held || held & 1 << root == 0 {
                continue; // must contain every held note, and its root must be held
            }
            let rank = (m != held, shape.len(), root != bass, i);
            if best.as_ref().map_or(true, |(r, _)| rank < *r) {
                best = Some((rank, (root, i as u8)));
            }
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: u8 = 48; // C3

    #[test]
    fn recognises_chords_from_held_keys() {
        assert_eq!(detect(&[C, C + 4, C + 7]), Some((0, MAJ)));
        assert_eq!(detect(&[C + 9, C + 12, C + 16]), Some((9, MIN)), "A minor");
        assert_eq!(detect(&[C + 4, C + 7, C + 12]), Some((0, MAJ)), "inversion: still C major");
        assert_eq!(detect(&[C + 7, C + 11, C + 14, C + 17]), Some((7, DOM7)), "G7");
        assert_eq!(detect(&[C, C + 4, C + 7, C + 11]), Some((0, MAJ7)));
        assert_eq!(detect(&[C + 2, C + 5, C + 9, C + 12]), Some((2, MIN7)), "Dm7, not F6");
        assert_eq!(detect(&[C, C + 4]), Some((0, MAJ)), "a third reads as the triad");
        assert_eq!(detect(&[C, C + 7]), Some((0, 10)), "a fifth is a power chord");
        assert_eq!(detect(&[C, C + 1, C + 2]), None, "a cluster isn't a chord");
        assert_eq!(detect(&[]), None);
    }

    fn on(improv: &Improv, c: Command) {
        improv.command(c).unwrap();
    }

    #[test]
    fn lead_lights_chord_tones_bright_and_the_scale_dim_above_the_split() {
        let im = Improv::new();
        on(&im, Command { active: Some(true), ..Default::default() });
        let mut out = vec![];
        let held = |k: u8| [C - 3, C, C + 4].contains(&k); // A minor (A2 C3 E3) in C major
        let h = im.tick(10.0, held, &mut |m| out.push(m)).unwrap();
        assert_eq!(h[69], HINT_LEFT, "A4: chord tone");
        assert_eq!(h[72], HINT_LEFT, "C5: chord tone");
        assert_eq!(h[62], HINT_RIGHT | HINT_PREVIEW, "D4: in the scale");
        assert_eq!(h[61], 0, "C#4: outside the key");
        assert_eq!(h[57], 0, "below the split: the chord hand's own keys");
        assert!(out.is_empty(), "lead mode plays nothing");
        // Letting go keeps the chord.
        let h = im.tick(10.0, |_| false, &mut |m| out.push(m)).unwrap();
        assert_eq!((h[69], im.status().chord), (HINT_LEFT, Some((9, MIN))));
        // Pentatonic: F and B drop out.
        on(&im, Command { pentatonic: Some(true), ..Default::default() });
        let h = im.tick(10.0, |_| false, &mut |m| out.push(m)).unwrap();
        assert_eq!((h[65], h[71], h[62]), (0, 0, HINT_RIGHT | HINT_PREVIEW));
    }

    #[test]
    fn backing_plays_one_chord_per_bar_in_the_key() {
        let im = Improv::new();
        on(&im, Command { active: Some(true), source: Some(Source::Backing), key: Some(7), tempo: Some(120), ..Default::default() });
        let mut out = vec![];
        let mut run = |ms: u32| {
            for _ in 0..ms / 10 {
                im.tick(10.0, |_| false, &mut |m| out.push(m));
            }
        };
        run(10);
        // G major (I) first, rooted on G2.
        assert_eq!(im.status().chord, Some((7, MAJ)));
        assert_eq!(im.status().next, Some((2, MAJ)), "then D (V)");
        run(2000); // one bar at 120 bpm
        assert_eq!(im.status().chord, Some((2, MAJ)));
        assert_eq!(&out[..3], &[[0x90, 43, VELOCITY], [0x90, 47, VELOCITY], [0x90, 50, VELOCITY]]);
        assert_eq!(&out[3..6], &[[0x80, 43, 0], [0x80, 47, 0], [0x80, 50, 0]], "the old chord is released");
        assert_eq!(out[6], [0x90, 50, VELOCITY], "D3");
        // Switching off releases the chord.
        on(&im, Command { active: Some(false), ..Default::default() });
        let n = out.len();
        im.tick(10.0, |_| false, &mut |m| out.push(m));
        assert_eq!(out[n..].iter().filter(|m| m[0] == 0x80).count(), 3);
        assert!(im.tick(10.0, |_| false, &mut |_| {}).is_none());
    }

    #[test]
    fn a_progression_sets_its_scale_and_bad_values_are_refused() {
        let im = Improv::new();
        on(&im, Command { progression: Some(Progression::Blues), ..Default::default() });
        assert_eq!(im.status().scale, Scale::Blues);
        assert!(im.command(Command { key: Some(12), ..Default::default() }).is_err());
        assert!(im.command(Command { tempo: Some(10), ..Default::default() }).is_err());
    }
}
