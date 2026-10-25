//! MIDI note number -> physical LED index mapping.
//!
//! The mapping is resolved once into a fixed 128-entry table, so the render
//! loop does one array load per note instead of re-running the float maths of
//! the original `get_note_position()` for every key on every frame. The table
//! is immutable after construction and therefore trivially `Sync`: share it
//! by reference (or `Arc`) between threads with no locking at all.

/// Strip geometry, mirroring the knobs of the Python implementation.
#[derive(Clone, Debug)]
pub struct StripGeometry {
    /// Total number of LEDs on the strip.
    pub led_count: u16,
    /// Strip density. The original treats 72 LEDs/m as "1 LED per semitone".
    pub leds_per_meter: u16,
    /// Global shift in LEDs (positive moves notes towards index 0).
    pub shift: i32,
    /// Strip is mounted with LED 0 at the treble end.
    pub reverse: bool,
    /// `(threshold, offset)`: every note *above* `threshold` gets `offset`
    /// LEDs subtracted. Offsets are cumulative, used to correct drift where
    /// strip segments are joined.
    pub note_offsets: Vec<(u8, i32)>,
    /// How many consecutive LEDs a single key lights (1 = original behaviour).
    pub span: u8,
}

impl Default for StripGeometry {
    fn default() -> Self {
        StripGeometry {
            led_count: 176,
            leds_per_meter: 144,
            shift: 0,
            reverse: false,
            note_offsets: Vec::new(),
            span: 1,
        }
    }
}

const UNMAPPED: u16 = u16::MAX;

#[derive(Clone)]
pub struct KeyMap {
    start: [u16; 128],
    span: u8,
    led_count: u16,
    reverse: bool,
}

impl KeyMap {
    pub fn new(g: &StripGeometry) -> Self {
        let mut start = [UNMAPPED; 128];
        let density = g.leds_per_meter as f32 / 72.0;
        let span = g.span.max(1);
        for note in 0u8..128 {
            let mut note_offset: i32 = 0;
            for &(threshold, offset) in &g.note_offsets {
                if note > threshold {
                    note_offset += offset;
                }
            }
            note_offset -= g.shift;
            // Python's int() truncates toward zero; `as i32` on f32 does too.
            let raw = (density * (note as f32 - 20.0) - note_offset as f32) as i32;
            // Same as the original (`led_number - pos` when reversed), so
            // shift/offset values calibrated there carry over unchanged.
            let pos = if g.reverse { g.led_count as i32 - raw } else { raw };
            // Unlike the original (which clamps out-of-range notes onto LED 0)
            // we leave them dark, so e.g. notes below A0 don't pile up on the
            // first LED.
            if pos < 0 || pos >= g.led_count as i32 {
                continue;
            }
            start[note as usize] = pos as u16;
        }
        KeyMap {
            start,
            span,
            led_count: g.led_count,
            reverse: g.reverse,
        }
    }

    /// First LED index for `note`, if it lands on the strip.
    #[inline(always)]
    pub fn led_for(&self, note: u8) -> Option<u16> {
        match self.start[(note & 0x7f) as usize] {
            UNMAPPED => None,
            i => Some(i),
        }
    }

    /// LED index range lit by `note`, clipped to the strip. When reversed the
    /// span grows towards index 0 so the key still extends to the treble side.
    #[inline(always)]
    pub fn leds_for(&self, note: u8) -> core::ops::Range<usize> {
        let Some(s) = self.led_for(note) else { return 0..0 };
        let s = s as usize;
        let span = self.span as usize;
        if self.reverse {
            s.saturating_sub(span - 1)..s + 1
        } else {
            s..(s + span).min(self.led_count as usize)
        }
    }

    pub fn led_count(&self) -> u16 {
        self.led_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_original_formula_144() {
        let km = KeyMap::new(&StripGeometry::default());
        // A0 (21) -> int(2 * 1) = 2, C8 (108) -> int(2 * 88) = 176 (off strip)
        assert_eq!(km.led_for(21), Some(2));
        assert_eq!(km.led_for(60), Some(80));
        assert_eq!(km.led_for(107), Some(174));
        assert_eq!(km.led_for(108), None);
        assert_eq!(km.led_for(19), None);
    }

    #[test]
    fn shift_and_offsets() {
        let g = StripGeometry {
            shift: 2,
            note_offsets: vec![(60, 1), (90, 1)],
            ..Default::default()
        };
        let km = KeyMap::new(&g);
        // shift=2 -> note_offset=-2 -> pos = 2*(n-20) + 2
        assert_eq!(km.led_for(21), Some(4));
        // note 61 > 60: note_offset = 1 - 2 = -1 -> 82 + 1
        assert_eq!(km.led_for(61), Some(83));
        // note 91 > 60, > 90: note_offset = 2 - 2 = 0 -> 142
        assert_eq!(km.led_for(91), Some(142));
    }

    #[test]
    fn reverse_is_mirror() {
        let fwd = KeyMap::new(&StripGeometry::default());
        let rev = KeyMap::new(&StripGeometry { reverse: true, ..Default::default() });
        for n in 0..128u8 {
            // Original formula: reversed = led_count - forward position.
            match (fwd.led_for(n), rev.led_for(n)) {
                (Some(a), Some(b)) => assert_eq!(a + b, 176),
                (Some(0), None) | (None, Some(0)) | (None, None) => {}
                other => panic!("note {n}: {other:?}"),
            }
        }
    }

    #[test]
    fn span_is_clipped() {
        let km = KeyMap::new(&StripGeometry { span: 3, ..Default::default() });
        assert_eq!(km.leds_for(107), 174..176);
        assert_eq!(km.leds_for(21), 2..5);
        assert_eq!(km.leds_for(0), 0..0);
    }
}
