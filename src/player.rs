//! Synthesia-style song player, running on the Pi so timing and wait-mode key
//! checks don't depend on the tablet or WiFi.
//!
//! A song is flattened once at load into notes sorted by start time. The
//! player thread advances a millisecond clock, sends the auto-played hand(s)
//! to the instrument, and publishes per-key hints for the LED strip.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use serde::{Deserialize, Serialize};

use crate::midi::MidiOut;
use crate::notes::{NoteState, HINT_BLINK, HINT_BRIGHT, HINT_LEFT, HINT_PREVIEW_SHIFT, HINT_RIGHT};

/// Notes starting within this window count as one chord in wait mode.
const CHORD_MS: u32 = 60;
/// Silence before the first note so falling notes can come into view.
const LEAD_IN_MS: f64 = 2000.0;
/// A press up to this long (song time) before a note counts for it, and keys
/// to play light up this early, so slightly early playing is judged right.
const EARLY_MS: f64 = 250.0;
/// Upcoming notes glow faintly on the strip this far ahead.
const PREVIEW_MS: f64 = 2000.0;
/// Waiting this long on a chord makes its unpressed keys blink.
const BLINK_AFTER_MS: f64 = 3000.0;
/// Default hand reach in semitones (C3..E4, a tenth).
pub const DEFAULT_REACH: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hand {
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Practise {
    Right,
    Left,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The instrument plays everything.
    Listen,
    /// You play the practised hand at tempo; the instrument plays the rest.
    Other,
    /// Like `Other`, but the song waits until you press each chord.
    Wait,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub t: u32,
    pub dur: u32,
    pub key: u8,
    pub vel: u8,
    pub track: u8,
    /// Written note value for sheet music, see [`note_value`].
    pub value: u8,
}

/// Classifies a length in beats (quarter notes) as the nearest written note
/// value, compared on a log scale. Returns `index * 2 + dotted`, with index
/// 0 = whole, 1 = half, 2 = quarter, 3 = eighth, 4 = sixteenth.
pub fn note_value(beats: f64) -> u8 {
    let mut best = (f64::MAX, 4);
    for idx in 0..5u8 {
        for dotted in 0..2u8 {
            let len = 4.0 / f64::from(1u8 << idx) * if dotted == 1 { 1.5 } else { 1.0 };
            let err = (beats.max(1e-3) / len).ln().abs();
            if err < best.0 {
                best = (err, idx * 2 + dotted);
            }
        }
    }
    best.1
}

pub struct Song {
    pub notes: Vec<Note>,
    /// Sustain pedal (CC 64) changes as `(t_ms, value)`, time-sorted.
    pub pedal: Vec<(u32, u8)>,
    /// Bar line times in ms, from the time signature (4/4 if none).
    pub bars: Vec<u32>,
    pub track_names: Vec<String>,
    pub len_ms: u32,
    max_dur: u32,
}

/// Parses a Standard MIDI File into a flat, time-sorted note list.
pub fn parse(bytes: &[u8]) -> Result<Song, String> {
    let smf = Smf::parse(bytes).map_err(|e| format!("invalid MIDI file: {e}"))?;
    let ntracks = smf.tracks.len().min(255);
    let mut names = vec![String::new(); ntracks];

    // Tempo changes usually live in track 0 but apply to all tracks, so merge
    // every track's events onto one absolute-tick timeline first.
    let mut events = Vec::new();
    for (ti, track) in smf.tracks.iter().take(ntracks).enumerate() {
        let mut tick = 0u64;
        for e in track {
            tick += e.delta.as_int() as u64;
            match e.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => {
                    names[ti] = String::from_utf8_lossy(n).trim().to_string();
                }
                TrackEventKind::Meta(MetaMessage::Tempo(_) | MetaMessage::TimeSignature(..))
                | TrackEventKind::Midi { .. } => {
                    events.push((tick, ti as u8, e.kind));
                }
                _ => {}
            }
        }
    }
    events.sort_by_key(|e| e.0); // stable: keeps per-track order at equal ticks

    let fixed_ms_per_tick = match smf.header.timing {
        Timing::Timecode(fps, sub) => Some(1000.0 / (fps.as_f32() as f64 * sub.max(1) as f64)),
        Timing::Metrical(_) => None,
    };
    let tpb = match smf.header.timing {
        Timing::Metrical(t) => t.as_int().max(1) as f64,
        Timing::Timecode(..) => 1.0,
    };

    let mut tempo_us = 500_000.0; // 120 bpm until told otherwise
    let (mut last_tick, mut ms) = (0u64, 0.0f64);
    // Bars: 4/4 until a time signature says otherwise. Timecode files have no beats.
    let mut bars = Vec::new();
    let mut bar_ticks = (tpb * 4.0) as u64;
    let mut next_bar = 0u64;
    let metrical = fixed_ms_per_tick.is_none();
    // Open notes per (track, channel, key): start ms, velocity, start tick.
    let mut open: HashMap<(u8, u8, u8), Vec<(f64, u8, u64)>> = HashMap::new();
    let mut notes = Vec::new();
    let mut pedal = Vec::new();
    let close = |notes: &mut Vec<Note>, track, key, (start, vel, start_tick): (f64, u8, u64), end: f64, end_tick: u64| {
        // Length in beats from ticks (tempo-independent); timecode files have
        // no beats, so assume 120 bpm there.
        let beats = match fixed_ms_per_tick {
            None => (end_tick - start_tick) as f64 / tpb,
            Some(_) => (end - start) / 500.0,
        };
        let dur = ((end - start) as u32).max(1);
        notes.push(Note { t: start as u32, dur, key, vel, track, value: note_value(beats) });
    };

    for (tick, track, kind) in events {
        while metrical && next_bar <= tick && bar_ticks > 0 {
            bars.push((ms + (next_bar - last_tick) as f64 * tempo_us / tpb / 1000.0) as u32);
            next_bar += bar_ticks;
        }
        let dt = (tick - last_tick) as f64;
        ms += fixed_ms_per_tick.map_or(dt * tempo_us / tpb / 1000.0, |m| dt * m);
        last_tick = tick;
        match kind {
            TrackEventKind::Meta(MetaMessage::Tempo(t)) => tempo_us = t.as_int().max(1) as f64,
            TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_pow, ..)) => {
                // A new signature starts a new bar here.
                bar_ticks = (tpb * 4.0 * num as f64 / f64::from(1u32 << den_pow.min(6))) as u64;
                if bars.last() != Some(&(ms as u32)) {
                    bars.push(ms as u32);
                }
                next_bar = tick + bar_ticks;
            }
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                if ch == 9 {
                    continue; // GM drums would light random keys
                }
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        open.entry((track, ch, key.as_int())).or_default().push((ms, vel.as_int(), tick));
                    }
                    MidiMessage::Controller { controller, value } if controller.as_int() == 64 => {
                        pedal.push((ms as u32, value.as_int()));
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        if let Some(stack) = open.get_mut(&(track, ch, key.as_int())) {
                            if !stack.is_empty() {
                                let started = stack.remove(0);
                                close(&mut notes, track, key.as_int(), started, ms, tick);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for ((track, _, key), stack) in open {
        for started in stack {
            close(&mut notes, track, key, started, ms, last_tick);
        }
    }
    notes.sort_by_key(|n| (n.t, n.key));
    if notes.is_empty() {
        return Err("song has no notes".into());
    }
    let len_ms = notes.iter().map(|n| n.t + n.dur).max().unwrap_or(0);
    let max_dur = notes.iter().map(|n| n.dur).max().unwrap_or(0);
    bars.dedup();
    Ok(Song { notes, pedal, bars, track_names: names, len_ms, max_dur })
}

/// What a track contributes. At most one track plays each hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackRole {
    Right,
    Left,
    /// Both hands in one track, split at middle C.
    Split,
    /// Not shown, not played, no LEDs.
    Ignore,
}

/// Default role per track: the highest-pitched note track is the right hand,
/// the lowest the left hand, and any others are ignored. When one track holds
/// (nearly) all notes it's both hands in one track (split), the rest ignored.
pub fn default_track_roles(song: &Song) -> Vec<TrackRole> {
    let n = song.track_names.len();
    let mut sum = vec![(0u64, 0u64); n];
    for note in &song.notes {
        let s = &mut sum[note.track as usize];
        s.0 += note.key as u64;
        s.1 += 1;
    }
    let with_notes: Vec<usize> = (0..n).filter(|&i| sum[i].1 > 0).collect();
    let total: u64 = sum.iter().map(|s| s.1).sum();
    let biggest = sum.iter().map(|s| s.1).max().unwrap_or(0);
    if with_notes.len() < 2 || biggest * 100 >= total * 85 {
        let main = (0..n).max_by_key(|&i| sum[i].1).unwrap_or(0);
        return (0..n).map(|i| if i == main { TrackRole::Split } else { TrackRole::Ignore }).collect();
    }
    let avg = |i: usize| sum[i].0 as f64 / sum[i].1 as f64;
    let by_pitch = |a: &usize, b: &usize| avg(*a).total_cmp(&avg(*b));
    let top = with_notes.iter().copied().max_by(by_pitch).unwrap();
    let bottom = with_notes.iter().copied().filter(|&i| i != top).min_by(by_pitch).unwrap();
    (0..n)
        .map(|i| match i {
            _ if i == top => TrackRole::Right,
            _ if i == bottom => TrackRole::Left,
            _ => TrackRole::Ignore,
        })
        .collect()
}

#[derive(Serialize)]
pub struct Status {
    pub song: Option<String>,
    pub playing: bool,
    pub waiting: bool,
    pub pos: i64,
    pub len: u32,
    pub mode: Mode,
    pub hand: Practise,
    pub tempo: u16,
    pub loop_ab: Option<[i64; 2]>,
}

/// Partial update from the UI; absent fields are left alone.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Command {
    pub playing: Option<bool>,
    pub mode: Option<Mode>,
    pub hand: Option<Practise>,
    pub tempo: Option<u16>,
    pub seek: Option<f64>,
    /// `[a, b]` in ms to loop, `[]` to clear.
    pub loop_ab: Option<Vec<f64>>,
    pub track_roles: Option<Vec<TrackRole>>,
    /// Widest stretch of one hand in semitones; 0 disables the filter.
    pub reach: Option<u8>,
}

struct State {
    song: Option<Arc<Song>>,
    name: Option<String>,
    track_roles: Vec<TrackRole>,
    mode: Mode,
    practise: Practise,
    tempo: u16,
    playing: bool,
    waiting: bool,
    pos: f64,
    loop_ab: Option<(f64, f64)>,
    reach: u8,
    /// Per note: out of reach for one hand, so the instrument plays it.
    unreachable: Vec<bool>,
    /// Reset on the next tick (after seek/load/play): release the
    /// instrument's keys and forget earlier key presses.
    flush: bool,
    /// Last seen press counter per key, and the song time of that press.
    seen: [u8; 128],
    pressed_at: [f64; 128],
    /// Press counter already used up by an accepted chord. A chord key needs
    /// an unused press from at most `EARLY_MS` before the note, so holding
    /// keys down can't walk through the song, but short (staccato) and
    /// slightly early presses still count.
    used: [u8; 128],
    /// End of the last accepted chord's window; its later notes are done.
    accepted_until: Option<u32>,
    /// Real time spent waiting on the current chord.
    wait_ms: f64,
    /// Chord keys still unpressed after `BLINK_AFTER_MS` of waiting.
    blink: [bool; 128],
    /// Keys the player currently holds down on the instrument.
    sounding: [bool; 128],
    /// Whether the player is holding the instrument's sustain pedal.
    pedal_down: bool,
    /// Reused per tick so the player thread doesn't allocate while playing.
    events: Vec<(f64, bool, u8, u8)>,
}

impl State {
    fn role(&self, n: &Note) -> TrackRole {
        self.track_roles.get(n.track as usize).copied().unwrap_or(TrackRole::Split)
    }

    fn ignored(&self, n: &Note) -> bool {
        self.role(n) == TrackRole::Ignore
    }

    fn hand_of(&self, n: &Note) -> Hand {
        match self.role(n) {
            TrackRole::Right => Hand::Right,
            TrackRole::Left => Hand::Left,
            _ if n.key >= 60 => Hand::Right,
            _ => Hand::Left,
        }
    }

    /// Recomputes which notes are out of reach. Per hand, notes starting
    /// within `CHORD_MS` form a chord; anything further than `reach` from the
    /// melody note (top for the right hand, bottom for the left) is dropped.
    /// Notes whose hand is only guessed (track split at middle C) are never
    /// dropped: the split says nothing about which hand really plays them.
    fn update_reach(&mut self) {
        let Some(song) = self.song.clone() else { return };
        let mut skip = vec![false; song.notes.len()];
        if self.reach > 0 {
            for hand in [Hand::Right, Hand::Left] {
                let known = |i: usize| matches!(self.role(&song.notes[i]), TrackRole::Right | TrackRole::Left);
                let idx: Vec<usize> = (0..song.notes.len())
                    .filter(|&i| known(i) && self.hand_of(&song.notes[i]) == hand)
                    .collect();
                let mut c = 0;
                while c < idx.len() {
                    let t0 = song.notes[idx[c]].t;
                    let end = idx[c..].iter().take_while(|&&i| song.notes[i].t <= t0 + CHORD_MS).count();
                    let chord = &idx[c..c + end];
                    let keys = chord.iter().map(|&i| song.notes[i].key);
                    let anchor = if hand == Hand::Right { keys.max() } else { keys.min() }.unwrap();
                    for &i in chord {
                        skip[i] = song.notes[i].key.abs_diff(anchor) > self.reach;
                    }
                    c += end;
                }
            }
        }
        self.unreachable = skip;
    }

    fn practised(&self, i: usize) -> bool {
        let Some(song) = &self.song else { return false };
        if self.unreachable.get(i).copied().unwrap_or(false) || self.ignored(&song.notes[i]) {
            return false;
        }
        match (self.mode, self.practise, self.hand_of(&song.notes[i])) {
            (Mode::Listen, ..) => false,
            (_, Practise::Both, _) => true,
            (_, Practise::Right, h) => h == Hand::Right,
            (_, Practise::Left, h) => h == Hand::Left,
        }
    }

    /// Releases every key (and the pedal) the player is holding.
    fn all_off(&mut self, out: &mut impl FnMut([u8; 3])) {
        for k in 0..128 {
            if std::mem::take(&mut self.sounding[k]) {
                out([0x80, k as u8, 0]);
            }
        }
        if std::mem::take(&mut self.pedal_down) {
            out([0xb0, 64, 0]);
        }
    }

    /// Whether key `k` has an unused press made at or after song time `since`.
    fn played(&self, k: u8, since: f64) -> bool {
        let k = k as usize;
        self.seen[k] != self.used[k] && self.pressed_at[k] >= since
    }

    /// Advances the clock by `dt` real milliseconds.
    /// `presses(k)` returns the wrapping press count of key `k`.
    fn tick(&mut self, dt: f64, presses: impl Fn(u8) -> u8, out: &mut impl FnMut([u8; 3])) {
        let Some(song) = self.song.clone() else { return };
        if std::mem::take(&mut self.flush) {
            self.all_off(out);
            for k in 0..128 {
                let p = presses(k as u8);
                (self.seen[k], self.used[k]) = (p, p);
            }
            self.accepted_until = None;
        }
        if !self.playing {
            self.all_off(out);
            return;
        }
        for k in 0..128 {
            let p = presses(k as u8);
            if p != self.seen[k] {
                self.seen[k] = p;
                self.pressed_at[k] = self.pos;
            }
        }
        let mut next = self.pos + dt * self.tempo as f64 / 100.0;
        self.waiting = false;

        if self.mode == Mode::Wait {
            // Stop just before the first chord in range whose keys aren't all down.
            let start = song.notes.partition_point(|n| (n.t as f64) <= self.pos);
            for i in start..song.notes.len() {
                let n = &song.notes[i];
                if n.t as f64 > next {
                    break;
                }
                if !self.practised(i) || self.accepted_until.is_some_and(|c| n.t <= c) {
                    continue;
                }
                let chord_end = n.t + CHORD_MS;
                let since = n.t as f64 - EARLY_MS;
                let chord = (i..song.notes.len()).take_while(|&j| song.notes[j].t <= chord_end);
                let ok = chord.clone().filter(|&j| self.practised(j)).all(|j| self.played(song.notes[j].key, since));
                if !ok {
                    next = n.t as f64 - 0.5;
                    self.waiting = true;
                    if self.wait_ms >= BLINK_AFTER_MS {
                        for j in chord {
                            let k = song.notes[j].key;
                            self.blink[k as usize] = self.practised(j) && !self.played(k, since);
                        }
                    }
                    break;
                }
                // Use up exactly this chord's presses; others stay for later chords.
                for j in chord {
                    if self.practised(j) {
                        let k = song.notes[j].key as usize;
                        self.used[k] = self.seen[k];
                    }
                }
                self.accepted_until = Some(chord_end);
            }
        }

        if self.waiting {
            self.wait_ms += dt;
        } else {
            self.wait_ms = 0.0;
            self.blink = [false; 128];
        }

        // Note on/off events of auto-played notes inside (pos, next].
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        let lo = self.pos - song.max_dur as f64;
        let first = song.notes.partition_point(|n| (n.t as f64) < lo);
        for i in first..song.notes.len() {
            let n = &song.notes[i];
            let t = n.t as f64;
            if t > next {
                break;
            }
            if self.practised(i) || self.ignored(n) {
                continue;
            }
            let end = t + n.dur as f64;
            if end > self.pos && end <= next {
                events.push((end, false, n.key, 0));
            }
            if t > self.pos && t <= next {
                events.push((t, true, n.key, n.vel));
            }
        }
        // Time order; at equal times note-offs first so repeated keys re-strike.
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, on, key, vel) in &events {
            let k = key as usize;
            if on {
                if self.sounding[k] {
                    out([0x80, key, 0]);
                }
                out([0x90, key, vel.max(1)]);
                self.sounding[k] = true;
            } else if std::mem::take(&mut self.sounding[k]) {
                out([0x80, key, 0]);
            }
        }
        self.events = events;

        // In Listen mode the song's pedal goes to the instrument too. While you
        // practise, your own foot owns the pedal.
        if self.mode == Mode::Listen {
            let first = song.pedal.partition_point(|&(t, _)| (t as f64) <= self.pos);
            for &(t, v) in &song.pedal[first..] {
                if t as f64 > next {
                    break;
                }
                out([0xb0, 64, v]);
                self.pedal_down = v >= 64;
            }
        } else if std::mem::take(&mut self.pedal_down) {
            out([0xb0, 64, 0]);
        }
        self.pos = next;

        if let Some((a, b)) = self.loop_ab {
            if self.pos >= b {
                self.pos = a;
                self.flush = true; // next tick: release keys, chords count afresh
            }
        }
        if self.pos > song.len_ms as f64 + 1000.0 {
            self.playing = false;
            self.all_off(out);
        }
    }

    fn hints(&self) -> [u8; 128] {
        let mut h = [0u8; 128];
        let Some(song) = &self.song else { return h };
        // While waiting, show the whole chord we're stopped in front of.
        let ahead = if self.waiting { CHORD_MS as f64 + 1.0 } else { EARLY_MS };
        let lo = self.pos - song.max_dur as f64;
        let first = song.notes.partition_point(|n| (n.t as f64) < lo);
        // Start time of the next upcoming note per hand [right, left]: only that
        // note (or chord) is previewed, so the strip never shows a queue.
        let mut next_onset: [Option<u32>; 2] = [None, None];
        for i in first..song.notes.len() {
            let n = &song.notes[i];
            let t = n.t as f64;
            if t > self.pos + PREVIEW_MS {
                break;
            }
            if self.ignored(n) {
                continue;
            }
            let hand = if self.hand_of(n) == Hand::Right { HINT_RIGHT } else { HINT_LEFT };
            if t > self.pos + ahead {
                // Not started yet: faint preview, brighter as it gets closer.
                // Notes come in time order, so a key that's already lit keeps its hint.
                let onset = next_onset[(hand == HINT_LEFT) as usize].get_or_insert(n.t);
                if n.t <= *onset + CHORD_MS && h[n.key as usize] == 0 {
                    let closeness = 1.0 - (t - self.pos) / PREVIEW_MS;
                    let level = (1.0 + 14.0 * closeness).clamp(1.0, 15.0) as u8;
                    h[n.key as usize] = hand | level << HINT_PREVIEW_SHIFT;
                }
                continue;
            }
            if self.practised(i) {
                if self.pos < t + n.dur as f64 {
                    let blink = if self.blink[n.key as usize] { HINT_BLINK } else { 0 };
                    h[n.key as usize] = hand | blink;
                }
            } else if t <= self.pos && self.sounding[n.key as usize] {
                h[n.key as usize] = hand | HINT_BRIGHT;
            }
        }
        h
    }
}

pub struct Player {
    state: Mutex<State>,
}

impl Player {
    pub fn new() -> Self {
        Player {
            state: Mutex::new(State {
                song: None,
                name: None,
                track_roles: Vec::new(),
                mode: Mode::Wait,
                practise: Practise::Right,
                tempo: 100,
                playing: false,
                waiting: false,
                pos: -LEAD_IN_MS,
                loop_ab: None,
                reach: DEFAULT_REACH,
                unreachable: Vec::new(),
                flush: false,
                seen: [0; 128],
                pressed_at: [f64::NEG_INFINITY; 128],
                used: [0; 128],
                accepted_until: None,
                wait_ms: 0.0,
                blink: [false; 128],
                sounding: [false; 128],
                pedal_down: false,
                events: Vec::with_capacity(64),
            }),
        }
    }

    pub fn load(&self, name: String, song: Song) {
        let mut s = self.state.lock().unwrap();
        s.track_roles = default_track_roles(&song);
        s.song = Some(Arc::new(song));
        s.name = Some(name);
        s.playing = false;
        s.flush = true;
        s.pos = -LEAD_IN_MS;
        s.loop_ab = None;
        s.update_reach();
    }

    pub fn command(&self, c: Command) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if let Some(roles) = c.track_roles {
            if roles.len() != s.track_roles.len() {
                return Err("track_roles length mismatch".into());
            }
            s.track_roles = roles;
            s.update_reach();
        }
        if let Some(r) = c.reach {
            s.reach = r.min(48);
            s.update_reach();
        }
        if let Some(m) = c.mode {
            s.mode = m;
        }
        if let Some(h) = c.hand {
            s.practise = h;
        }
        if let Some(t) = c.tempo {
            s.tempo = t.clamp(10, 200);
        }
        let len = s.song.as_ref().map_or(0.0, |song| song.len_ms as f64);
        if let Some(p) = c.seek {
            s.pos = p.clamp(-LEAD_IN_MS, len);
            s.flush = true;
        }
        if let Some(ab) = c.loop_ab {
            s.loop_ab = match ab[..] {
                [a, b] if a < b => Some((a.clamp(-LEAD_IN_MS, len), b.clamp(-LEAD_IN_MS, len))),
                [] => None,
                _ => return Err("loop_ab must be [a, b] with a < b, or []".into()),
            };
        }
        if let Some(p) = c.playing {
            let start = p && !s.playing;
            s.playing = p && s.song.is_some();
            s.flush |= start;
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap();
        Status {
            song: s.name.clone(),
            playing: s.playing,
            waiting: s.waiting,
            pos: s.pos as i64,
            len: s.song.as_ref().map_or(0, |song| song.len_ms),
            mode: s.mode,
            hand: s.practise,
            tempo: s.tempo,
            loop_ab: s.loop_ab.map(|(a, b)| [a as i64, b as i64]),
        }
    }

    /// Current song for the UI: track list plus the flat note list.
    pub fn song_json(&self) -> Option<String> {
        #[derive(Serialize)]
        struct Track<'a> {
            name: &'a str,
            notes: usize,
            role: TrackRole,
        }
        #[derive(Serialize)]
        struct SongView<'a> {
            name: &'a str,
            len: u32,
            tracks: Vec<Track<'a>>,
            /// `[t, dur, key, hand(0=R,1=L), out_of_reach(0/1), note_value]`
            notes: Vec<(u32, u32, u8, u8, u8, u8)>,
            bars: &'a [u32],
        }
        let s = self.state.lock().unwrap();
        let song = s.song.as_ref()?;
        let mut counts = vec![0usize; song.track_names.len()];
        for n in &song.notes {
            counts[n.track as usize] += 1;
        }
        let view = SongView {
            name: s.name.as_deref().unwrap_or(""),
            len: song.len_ms,
            tracks: song
                .track_names
                .iter()
                .zip(&counts)
                .zip(&s.track_roles)
                .map(|((name, &notes), &role)| Track { name, notes, role })
                .collect(),
            // Ignored tracks are left out entirely.
            notes: song
                .notes
                .iter()
                .enumerate()
                .filter(|(_, n)| !s.ignored(n))
                .map(|(i, n)| {
                    let far = s.unreachable.get(i).copied().unwrap_or(false);
                    (n.t, n.dur, n.key, (s.hand_of(n) == Hand::Left) as u8, far as u8, n.value)
                })
                .collect(),
            bars: &song.bars,
        };
        serde_json::to_string(&view).ok()
    }
}

/// Player thread: ~3 ms ticks while playing, slower when idle.
pub fn run(
    player: Arc<Player>,
    notes: Arc<NoteState>,
    out: Arc<Mutex<Option<MidiOut>>>,
    running: Arc<AtomicBool>,
) {
    let mut last = Instant::now();
    let mut shown = [0u8; 128];
    while running.load(Ordering::Relaxed) {
        let (hints, playing) = {
            let mut s = player.state.lock().unwrap();
            let now = Instant::now();
            let dt = (now - last).as_secs_f64() * 1000.0;
            last = now;
            let mut o = out.lock().unwrap();
            let mut send = |m: [u8; 3]| {
                if m[0] == 0x90 {
                    notes.strike_hint(m[1]);
                }
                if let Some(o) = o.as_mut() {
                    o.send(&m);
                }
            };
            s.tick(dt, |k| notes.presses(k as usize), &mut send);
            notes.set_practice(s.playing && s.song.is_some() && s.mode != Mode::Listen);
            notes.set_playing(s.playing);
            (s.hints(), s.playing)
        };
        for (k, (&new, old)) in hints.iter().zip(shown.iter_mut()).enumerate() {
            if new != *old {
                notes.set_hint(k, new);
                *old = new;
            }
        }
        std::thread::sleep(Duration::from_millis(if playing { 3 } else { 30 }));
    }
    let mut s = player.state.lock().unwrap();
    let mut o = out.lock().unwrap();
    s.all_off(&mut |m| {
        if let Some(o) = o.as_mut() {
            o.send(&m);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(notes: &[(u32, u32, u8, u8)]) -> Song {
        let notes: Vec<Note> = notes
            .iter()
            .map(|&(t, dur, key, track)| Note { t, dur, key, vel: 80, track, value: 4 })
            .collect();
        let mut notes = notes;
        notes.sort_by_key(|n| (n.t, n.key)); // same invariant as `parse`
        let len_ms = notes.iter().map(|n| n.t + n.dur).max().unwrap();
        let max_dur = notes.iter().map(|n| n.dur).max().unwrap();
        Song { notes, pedal: vec![(0, 127), (150, 0)], bars: vec![], track_names: vec!["R".into(), "L".into()], len_ms, max_dur }
    }

    fn player_with(s: Song, mode: Mode) -> Player {
        let p = Player::new();
        p.load("t".into(), s);
        p.command(Command { mode: Some(mode), playing: Some(true), ..Default::default() }).unwrap();
        p
    }

    /// Runs `ms` with each of `pressed` pressed once (press count 1).
    fn run_ms(p: &Player, ms: u32, pressed: &[u8], out: &mut Vec<[u8; 3]>) {
        run_with(p, ms, |k| pressed.contains(&k) as u8, out);
    }

    fn run_with(p: &Player, ms: u32, key: impl Fn(u8) -> u8, out: &mut Vec<[u8; 3]>) {
        let mut s = p.state.lock().unwrap();
        for _ in 0..ms / 5 {
            s.tick(5.0, &key, &mut |m| out.push(m));
        }
    }

    #[test]
    fn listen_plays_everything_in_order() {
        let p = player_with(song(&[(0, 100, 60, 0), (50, 100, 40, 1)]), Mode::Listen);
        let mut out = vec![];
        run_ms(&p, 2500, &[], &mut out);
        assert_eq!(
            out,
            vec![[0x90, 60, 80], [0xb0, 64, 127], [0x90, 40, 80], [0x80, 60, 0], [0x80, 40, 0], [0xb0, 64, 0]]
        );
    }

    #[test]
    fn other_hand_only_plays_the_other_hand() {
        let p = player_with(song(&[(0, 100, 72, 0), (0, 100, 40, 1)]), Mode::Other);
        let mut out = vec![];
        run_ms(&p, 2500, &[], &mut out);
        // Track 0 is the higher one -> right hand (practised); track 1 is auto.
        assert_eq!(out, vec![[0x90, 40, 80], [0x80, 40, 0]]);
    }

    #[test]
    fn wait_mode_stops_until_chord_is_held() {
        let p = player_with(song(&[(0, 500, 72, 0), (10, 500, 76, 0), (0, 400, 40, 1)]), Mode::Wait);
        let mut out = vec![];
        run_ms(&p, 3000, &[], &mut out);
        let st = p.status();
        assert!(st.waiting && st.pos <= 0, "{}", st.pos);
        assert!(out.is_empty(), "left hand must wait too: {out:?}");
        // Hints show the whole chord in the right-hand colour.
        let h = p.state.lock().unwrap().hints();
        assert_eq!((h[72], h[76]), (HINT_RIGHT, HINT_RIGHT));

        run_ms(&p, 100, &[72], &mut out); // only half the chord
        assert!(p.status().waiting);
        run_ms(&p, 100, &[72, 76], &mut out);
        assert!(!p.status().waiting);
        assert_eq!(out.first(), Some(&[0x90, 40, 80]));
    }

    #[test]
    fn out_of_reach_notes_are_auto_played() {
        // Right-hand chord C4 + E5 + C6: C6 is the melody, C4 is 24 semitones away.
        let p = player_with(song(&[(0, 100, 60, 0), (0, 100, 76, 0), (0, 100, 84, 0), (0, 100, 30, 1)]), Mode::Wait);
        let mut out = vec![];
        run_ms(&p, 1900, &[], &mut out); // lead-in, nothing pressed yet
        run_ms(&p, 300, &[76, 84], &mut out); // pressed 100 ms before the chord
        assert!(!p.status().waiting, "C4 must not be required");
        assert!(out.contains(&[0x90, 60, 80]), "piano plays the unreachable note: {out:?}");
        // reach 0 turns the filter off: now C4 is required again.
        p.command(Command { reach: Some(0), seek: Some(-100.0), ..Default::default() }).unwrap();
        run_ms(&p, 200, &[76, 84], &mut out);
        assert!(p.status().waiting);
    }

    #[test]
    fn holding_keys_down_does_not_advance() {
        // The same right-hand note twice in a row.
        let p = player_with(song(&[(0, 200, 72, 0), (500, 200, 72, 0), (0, 900, 40, 1)]), Mode::Wait);
        let mut out = vec![];
        // Keys held down since before playback started don't count.
        run_with(&p, 2500, |_| 0, &mut out);
        assert!(p.status().waiting && p.status().pos <= 0);
        // One press: first note accepted, the repeat waits for a new press.
        run_with(&p, 1000, |k| (k == 72) as u8, &mut out);
        let st = p.status();
        assert!(st.waiting && (450..=500).contains(&st.pos), "{}", st.pos);
        run_with(&p, 200, |k| if k == 72 { 2 } else { 0 }, &mut out);
        assert!(!p.status().waiting);
    }

    #[test]
    fn early_and_staccato_presses_count() {
        // Melody 72, 74, 76 every 300 ms; the player is a little ahead of the clock.
        let p = player_with(song(&[(0, 100, 72, 0), (300, 100, 74, 0), (600, 100, 76, 0), (0, 900, 40, 1)]), Mode::Wait);
        let mut out = vec![];
        run_ms(&p, 1900, &[], &mut out); // pos -100
        // 72 pressed before its note (held or not doesn't matter)...
        run_ms(&p, 200, &[72], &mut out); // pos 100
        // ...and 74 pressed 200 ms ahead of its note, before the clock gets there.
        run_with(&p, 600, |k| matches!(k, 72 | 74) as u8, &mut out);
        let st = p.status();
        assert!(st.waiting && (550..=600).contains(&st.pos), "both counted, waits at 76: {}", st.pos);
        // A press from long before the 250 ms window does not count.
        let p = player_with(song(&[(1000, 100, 72, 0)]), Mode::Wait);
        run_ms(&p, 1000, &[72], &mut out);
        run_ms(&p, 2500, &[72], &mut out);
        assert!(p.status().waiting);
    }

    #[test]
    fn unpressed_chord_keys_blink_after_a_while() {
        let p = player_with(song(&[(0, 200, 72, 0), (0, 200, 76, 0)]), Mode::Wait);
        let mut out = vec![];
        run_ms(&p, 2500, &[], &mut out);
        let h = p.state.lock().unwrap().hints();
        assert_eq!(h[72] & HINT_BLINK, 0, "not yet");
        run_ms(&p, 3100, &[72], &mut out); // one key pressed, the other forgotten
        let h = p.state.lock().unwrap().hints();
        assert_eq!((h[72] & HINT_BLINK, h[76] & HINT_BLINK), (0, HINT_BLINK));
        run_ms(&p, 50, &[72, 76], &mut out);
        let h = p.state.lock().unwrap().hints();
        assert_eq!(h[76] & HINT_BLINK, 0, "blinking stops once played");
    }

    #[test]
    fn keys_to_play_light_up_slightly_early() {
        let p = player_with(song(&[(1000, 200, 72, 0)]), Mode::Other);
        let mut out = vec![];
        run_ms(&p, 2900, &[], &mut out); // pos ~ 900
        assert_eq!(p.state.lock().unwrap().hints()[72], HINT_RIGHT);
    }

    #[test]
    fn pedal_only_in_listen_mode_and_released_on_pause() {
        let p = player_with(song(&[(0, 300, 72, 0)]), Mode::Listen);
        let mut out = vec![];
        run_ms(&p, 2100, &[], &mut out); // pos 100: pedal pressed at 0
        assert!(out.contains(&[0xb0, 64, 127]));
        p.command(Command { playing: Some(false), ..Default::default() }).unwrap();
        out.clear();
        run_ms(&p, 10, &[], &mut out);
        assert!(out.contains(&[0xb0, 64, 0]), "pause lifts the pedal: {out:?}");
        // Practising: the song's pedal is not sent.
        let p = player_with(song(&[(0, 300, 72, 0)]), Mode::Other);
        out.clear();
        run_ms(&p, 2500, &[], &mut out);
        assert!(!out.iter().any(|m| m[0] == 0xb0), "{out:?}");
    }

    #[test]
    fn one_dominant_track_is_split_and_not_reach_filtered() {
        // Track 0 has nearly everything (a wide two-hand chord), track 1 one stray note.
        let mut notes: Vec<(u32, u32, u8, u8)> = (0..20).map(|i| (i * 500, 400, 40, 0)).collect();
        notes.extend((0..20).map(|i| (i * 500, 400, 84, 0)));
        notes.push((0, 100, 70, 1));
        let s = song(&notes);
        assert_eq!(default_track_roles(&s), vec![TrackRole::Split, TrackRole::Ignore]);
        let p = player_with(s, Mode::Wait);
        let st = p.state.lock().unwrap();
        assert!(st.unreachable.iter().all(|&u| !u), "guessed hands: no reach filter");
        // Split at middle C gives both hands.
        assert_eq!(st.hand_of(&Note { t: 0, dur: 1, key: 40, vel: 1, track: 0, value: 4 }), Hand::Left);
        assert_eq!(st.hand_of(&Note { t: 0, dur: 1, key: 84, vel: 1, track: 0, value: 4 }), Hand::Right);
    }

    #[test]
    fn note_values_from_beats() {
        assert_eq!(note_value(4.0), 0); // whole
        assert_eq!(note_value(2.0), 2); // half
        assert_eq!(note_value(1.0), 4); // quarter
        assert_eq!(note_value(1.5), 5); // dotted quarter
        assert_eq!(note_value(0.5), 6); // eighth
        assert_eq!(note_value(0.23), 8); // sixteenth (slightly short, as played)
        assert_eq!(note_value(0.9), 4); // a held-short quarter is still a quarter
        assert_eq!(note_value(20.0), 1); // very long: dotted whole at most
    }

    #[test]
    fn extra_tracks_are_ignored_everywhere() {
        // Three tracks: high (right), low (left), middle (ignored by default).
        let mut s = song(&[(0, 300, 84, 0), (0, 300, 36, 1), (0, 300, 60, 2)]);
        s.track_names.push("Strings".into());
        assert_eq!(default_track_roles(&s), vec![TrackRole::Right, TrackRole::Left, TrackRole::Ignore]);
        let p = player_with(s, Mode::Listen);
        let mut out = vec![];
        run_ms(&p, 2500, &[], &mut out);
        assert!(!out.contains(&[0x90, 60, 80]), "ignored track is not played: {out:?}");
        assert!(out.contains(&[0x90, 84, 80]) && out.contains(&[0x90, 36, 80]));
        let json = p.song_json().unwrap();
        assert!(!json.contains("[0,300,60,"), "ignored notes not sent to the tablet");
        // Practising: ignored notes aren't waited for.
        let wait = Command { mode: Some(Mode::Wait), hand: Some(Practise::Both), seek: Some(-100.0), playing: Some(true), ..Default::default() };
        p.command(wait).unwrap();
        run_ms(&p, 50, &[], &mut out); // presses before the seek don't count
        run_ms(&p, 200, &[84, 36], &mut out);
        assert!(!p.status().waiting);
    }

    #[test]
    fn upcoming_notes_preview_gets_brighter() {
        let p = player_with(song(&[(1500, 200, 72, 0), (2500, 200, 74, 0), (9000, 200, 76, 0)]), Mode::Other);
        let mut out = vec![];
        run_ms(&p, 2000, &[], &mut out); // pos ~ 0
        let h = p.state.lock().unwrap().hints();
        let level = |v: u8| v >> HINT_PREVIEW_SHIFT;
        assert!(level(h[72]) > level(h[74]) && level(h[74]) == 0, "72 in 1.5 s, 74 beyond 2 s");
        assert!(level(h[72]) > 0 && h[72] & 3 == HINT_RIGHT);
        assert_eq!(h[76], 0, "far future: dark");
        run_ms(&p, 1400, &[], &mut out); // pos ~ 1400: 72 now inside the play window
        let h = p.state.lock().unwrap().hints();
        assert_eq!(h[72], HINT_RIGHT, "becomes a normal hint");
        assert!(level(h[74]) > 0);
    }

    #[test]
    fn preview_shows_only_the_next_note_or_chord_per_hand() {
        // Right hand: chord at 1000 (72+76), then 74 at 1500. Left hand: 40 at 1200, 43 at 1600.
        let p = player_with(
            song(&[(1000, 100, 72, 0), (1010, 100, 76, 0), (1500, 100, 74, 0), (1200, 100, 40, 1), (1600, 100, 43, 1)]),
            Mode::Other,
        );
        let mut out = vec![];
        run_ms(&p, 2000, &[], &mut out); // pos ~ 0
        let h = p.state.lock().unwrap().hints();
        let previewed = |k: usize| h[k] >> HINT_PREVIEW_SHIFT != 0;
        assert!(previewed(72) && previewed(76), "whole next right-hand chord");
        assert!(!previewed(74), "not the one after it");
        assert!(previewed(40) && !previewed(43), "left hand gets its own next note");
    }

    #[test]
    fn track_roles_survive_settings_and_reach_updates() {
        let mut s = song(&[(0, 300, 84, 0), (0, 300, 36, 1), (0, 300, 60, 2)]);
        s.track_names.push("Strings".into());
        let p = player_with(s, Mode::Wait);
        let roles = vec![TrackRole::Ignore, TrackRole::Split, TrackRole::Right];
        p.command(Command { track_roles: Some(roles.clone()), ..Default::default() }).unwrap();
        // What a settings save and the UI's controls send afterwards.
        p.command(Command { reach: Some(12), ..Default::default() }).unwrap();
        p.command(Command { tempo: Some(80), mode: Some(Mode::Other), hand: Some(Practise::Left), ..Default::default() }).unwrap();
        p.command(Command { seek: Some(0.0), playing: Some(true), ..Default::default() }).unwrap();
        let mut out = vec![];
        run_ms(&p, 500, &[], &mut out);
        assert_eq!(p.state.lock().unwrap().track_roles, roles);
        assert!(p.song_json().unwrap().contains("\"role\":\"split\""));
    }

    #[test]
    fn loop_and_end_of_song() {
        let p = player_with(song(&[(0, 100, 60, 0)]), Mode::Listen);
        p.command(Command { loop_ab: Some(vec![0.0, 50.0]), seek: Some(0.0), ..Default::default() }).unwrap();
        let mut out = vec![];
        run_ms(&p, 500, &[], &mut out);
        assert!(p.status().pos < 60);
        p.command(Command { loop_ab: Some(vec![]), ..Default::default() }).unwrap();
        run_ms(&p, 2000, &[], &mut out);
        assert!(!p.status().playing);
        assert!(p.command(Command { loop_ab: Some(vec![5.0, 1.0]), ..Default::default() }).is_err());
    }

    #[test]
    fn parses_a_midi_file_with_tempo() {
        use midly::num::{u15, u24, u28, u4, u7};
        use midly::{Format, Header, TrackEvent};
        let ev = |delta: u32, kind| TrackEvent { delta: u28::new(delta), kind };
        let on = |key: u8, vel: u8| TrackEventKind::Midi {
            channel: u4::new(0),
            message: MidiMessage::NoteOn { key: u7::new(key), vel: u7::new(vel) },
        };
        let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(480))));
        // Track 0: 60 bpm (1 s per beat), then one note of 1 beat at beat 1.
        smf.tracks.push(vec![
            ev(0, TrackEventKind::Meta(MetaMessage::Tempo(u24::new(1_000_000)))),
            ev(0, TrackEventKind::Meta(MetaMessage::TimeSignature(3, 2, 24, 8))),
            ev(480, on(64, 90)),
            ev(480 * 4, on(65, 90)),
            ev(480, on(65, 0)),
            ev(480, on(64, 0)),
            ev(0, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ]);
        // Track 1: low note at time 0, half a beat long; drums ignored.
        smf.tracks.push(vec![
            ev(0, TrackEventKind::Meta(MetaMessage::TrackName(b"Left"))),
            ev(0, on(36, 70)),
            ev(0, TrackEventKind::Midi { channel: u4::new(9), message: MidiMessage::NoteOn { key: u7::new(38), vel: u7::new(100) } }),
            ev(0, TrackEventKind::Midi { channel: u4::new(0), message: MidiMessage::Controller { controller: u7::new(64), value: u7::new(127) } }),
            ev(240, TrackEventKind::Midi { channel: u4::new(0), message: MidiMessage::NoteOff { key: u7::new(36), vel: u7::new(0) } }),
            ev(0, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ]);
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();

        let song = parse(&bytes).unwrap();
        assert_eq!(song.track_names[1], "Left");
        assert_eq!(
            song.notes,
            vec![
                // Half a beat = eighth (3 * 2), one beat = quarter (2 * 2).
                Note { t: 0, dur: 500, key: 36, vel: 70, track: 1, value: 6 },
                Note { t: 1000, dur: 6000, key: 64, vel: 90, track: 0, value: 1 }, // 6 beats: dotted whole
                Note { t: 5000, dur: 1000, key: 65, vel: 90, track: 0, value: 4 },
            ]
        );
        assert_eq!(song.pedal, vec![(0, 127)]);
        // 3/4 at 60 bpm: a bar every 3 s.
        assert_eq!(song.bars, vec![0, 3000, 6000]);
        assert_eq!(default_track_roles(&song), vec![TrackRole::Right, TrackRole::Left]);
        assert!(parse(b"not midi").is_err());
    }
}
