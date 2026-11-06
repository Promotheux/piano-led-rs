//! A rough difficulty rating for a song, from three things a player feels:
//! how many notes come per second, how wide one hand has to stretch, and
//! how fast the quickest passages go. Cheap enough to run on upload.

use serde::{Deserialize, Serialize};

use crate::player::{default_track_roles, Song, TrackRole};

/// Notes starting this close together are one chord (as in wait mode).
const CHORD_MS: u32 = 60;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Beginner,
    Easy,
    Intermediate,
    Advanced,
    Expert,
}

#[derive(Debug, PartialEq)]
pub struct Metrics {
    /// Notes per second, start to end.
    pub notes_per_sec: f32,
    /// Widest chord in one hand (semitones), ignoring the odd outlier.
    pub stretch: u8,
    /// Time between chords in the quick passages (ms; 10th percentile).
    pub fast_ms: u32,
}

/// The value at `pct`% of the sorted list (0 for an empty one).
fn percentile<T: Copy + Ord + Default>(mut v: Vec<T>, pct: usize) -> T {
    v.sort_unstable();
    v.get((v.len().saturating_sub(1)) * pct / 100).copied().unwrap_or_default()
}

pub fn measure(song: &Song) -> Metrics {
    let roles = default_track_roles(song);
    let hand = |track: u8, key: u8| match roles.get(track as usize) {
        Some(TrackRole::Ignore) => None,
        Some(TrackRole::Right) => Some(0),
        Some(TrackRole::Left) => Some(1),
        _ => Some(if key >= 60 { 0 } else { 1 }),
    };
    let (mut stretches, mut gaps) = (Vec::new(), Vec::new());
    let (mut count, mut first, mut last) = (0u32, u32::MAX, 0u32);
    for h in [0, 1] {
        // Chords of this hand: (start, lowest key, highest key).
        let mut chords: Vec<(u32, u8, u8)> = Vec::new();
        for n in song.notes.iter().filter(|n| hand(n.track, n.key) == Some(h)) {
            count += 1;
            (first, last) = (first.min(n.t), last.max(n.t));
            match chords.last_mut() {
                Some(c) if n.t <= c.0 + CHORD_MS => (c.1, c.2) = (c.1.min(n.key), c.2.max(n.key)),
                _ => chords.push((n.t, n.key, n.key)),
            }
        }
        stretches.extend(chords.iter().map(|c| c.2 - c.1));
        gaps.extend(chords.windows(2).map(|w| w[1].0 - w[0].0));
    }
    let secs = (last.saturating_sub(first) as f32 / 1000.0).max(1.0);
    Metrics {
        notes_per_sec: count as f32 / secs,
        stretch: percentile(stretches, 95),
        fast_ms: if gaps.is_empty() { 1000 } else { percentile(gaps, 10) },
    }
}

/// 0..=100: density counts most, then speed, then stretch.
pub fn score(m: &Metrics) -> f32 {
    let part = |x: f32| x.clamp(0.0, 1.0);
    40.0 * part((m.notes_per_sec - 1.0) / 9.0)
        + 25.0 * part((m.stretch as f32 - 7.0) / 9.0)
        + 35.0 * part((300.0 - m.fast_ms as f32) / 220.0)
}

pub fn level(m: &Metrics) -> Level {
    match score(m) {
        s if s < 15.0 => Level::Beginner,
        s if s < 35.0 => Level::Easy,
        s if s < 55.0 => Level::Intermediate,
        s if s < 75.0 => Level::Advanced,
        _ => Level::Expert,
    }
}

pub fn rate(song: &Song) -> Level {
    level(&measure(song))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_score() {
        let m = |notes_per_sec, stretch, fast_ms| Metrics { notes_per_sec, stretch, fast_ms };
        assert_eq!(level(&m(1.0, 4, 600)), Level::Beginner, "one slow note at a time");
        assert_eq!(level(&m(4.0, 12, 400)), Level::Easy);
        assert_eq!(level(&m(6.0, 12, 220)), Level::Intermediate);
        assert_eq!(level(&m(8.0, 12, 160)), Level::Advanced);
        assert_eq!(level(&m(12.0, 16, 100)), Level::Expert, "dense, wide and fast");
    }

    #[test]
    fn measures_a_parsed_file() {
        use midly::num::{u15, u28, u4, u7};
        use midly::{Format, Header, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
        let ev = |delta: u32, key: u8, on: bool| TrackEvent {
            delta: u28::new(delta),
            kind: TrackEventKind::Midi {
                channel: u4::new(0),
                message: MidiMessage::NoteOn { key: u7::new(key), vel: u7::new(if on { 80 } else { 0 }) },
            },
        };
        // C major chords (C4 E4 G4), one every half second (120 bpm quarters), 8 of them.
        let mut track = Vec::new();
        for _ in 0..8 {
            track.extend([ev(0, 60, true), ev(0, 64, true), ev(0, 67, true)]);
            track.extend([ev(480, 60, false), ev(0, 64, false), ev(0, 67, false)]);
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack) });
        let smf = Smf { header: Header::new(Format::SingleTrack, Timing::Metrical(u15::new(480))), tracks: vec![track] };
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes).unwrap();
        let m = measure(&crate::player::parse(&bytes).unwrap());
        assert_eq!((m.stretch, m.fast_ms), (7, 500));
        assert!((m.notes_per_sec - 24.0 / 3.5).abs() < 0.01, "{}", m.notes_per_sec);
        assert_eq!(level(&m), Level::Easy);
    }
}

/// Prints every song in the dev songs folder with its measures, for tuning
/// the score: `cargo test --release print_library -- --ignored --nocapture`.
#[cfg(test)]
mod calibrate {
    #[test]
    #[ignore]
    fn print_library() {
        let mut rows = Vec::new();
        for e in std::fs::read_dir("images/backup-songs").unwrap().flatten() {
            let name = e.file_name().into_string().unwrap();
            if !name.to_lowercase().ends_with(".mid") {
                continue;
            }
            let Ok(song) = crate::player::parse(&std::fs::read(e.path()).unwrap()) else { continue };
            let m = super::measure(&song);
            rows.push((super::score(&m), format!("{:5.1} {:12?} nps {:4.1} stretch {:2} fast {:4}  {}", super::score(&m), super::level(&m), m.notes_per_sec, m.stretch, m.fast_ms, name)));
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, r) in rows {
            println!("{r}");
        }
    }
}
