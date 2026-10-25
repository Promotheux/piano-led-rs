//! Frame renderer: note state -> per-key envelopes -> LED buffer -> driver.
//!
//! Owns every buffer it touches, all sized once in [`Renderer::new`]. The
//! per-frame path is integer-only and allocation-free.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::color::{GammaLut, Rgb};
use crate::hal::LedDriver;
use crate::keymap::KeyMap;
use crate::notes::{NoteState, HINT_BLINK, HINT_BRIGHT, HINT_PREVIEW_SHIFT};
use crate::settings::{Settings, SettingsStore};

/// Envelope level is 8.8 fixed point: 0..=MAX_LEVEL.
const MAX_LEVEL: u32 = 255 << 8;
/// Level of notes the player itself sounds; keys you should play get 255.
const HINT_AUTO: u8 = 160;
/// A key struck again while still lit dips to this level (of 255)...
const RESTRIKE_DIM: u8 = 64;
/// ...for this long, then jumps back to full, so repeated notes separate.
const RESTRIKE_GAP_US: u32 = 40_000;
/// Preview brightness range (of 255) for upcoming notes, far -> about to start.
const PREVIEW_MIN: u32 = 18;
const PREVIEW_MAX: u32 = 70;
/// Backlight turns off after this long with no keys pressed and no song playing.
const SCREENSAVER_US: u64 = 120_000_000;
/// Half period of the "you're taking long" blink (8 flashes per second).
const BLINK_HALF_US: u64 = 62_500;
/// Practice-mode feedback colours for keys you press.
const RIGHT_KEY: Rgb = Rgb::new(0, 255, 0);
const WRONG_KEY: Rgb = Rgb::new(255, 0, 0);

#[derive(Clone, Debug)]
pub struct RenderConfig {
    pub fps: u16,
    /// Release fade time after key up, full brightness -> off.
    pub fade: Duration,
    /// Scale brightness by note velocity.
    pub velocity: bool,
    /// Global brightness 0..=255 (folded into the gamma table).
    pub brightness: u8,
    pub gamma: f32,
    /// Supply current budget in mA, 0 = unlimited. Estimated with the usual
    /// WS2812B figures (~20 mA per channel at full, ~1 mA idle per LED).
    pub max_milliamps: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            fps: 60,
            fade: Duration::from_millis(350),
            velocity: true,
            brightness: 128,
            gamma: 2.2,
            max_milliamps: 0,
        }
    }
}

pub struct Renderer {
    cfg: RenderConfig,
    keymap: KeyMap,
    /// Base colour per MIDI note (precomputed from the colour mode).
    note_color: [Rgb; 128],
    /// Current envelope level per note.
    level: [u32; 128],
    /// Peak level for the current strike (velocity-scaled).
    peak: [u32; 128],
    /// Player hint colours: [right hand, left hand].
    hand_color: [Rgb; 2],
    /// Constant base colour under the keys (already scaled, no gamma).
    backlight: Rgb,
    /// Whether each note drew anything last frame (for re-strike detection).
    lit: [bool; 128],
    /// Remaining re-strike dip per note, in µs.
    gap: [u32; 128],
    /// Colour override per pressed key in practice modes (green/red).
    feedback: [Option<Rgb>; 128],
    /// Running time for the blink phase.
    clock_us: u64,
    /// Time since the last key press or playing song (backlight screensaver).
    idle_us: u64,
    pixels: Box<[Rgb]>,
    gamma: GammaLut,
    was_dark: bool,
}

impl Renderer {
    pub fn new(cfg: RenderConfig, keymap: KeyMap, note_color: [Rgb; 128]) -> Self {
        let gamma = GammaLut::new(cfg.brightness, cfg.gamma);
        let pixels = vec![Rgb::BLACK; keymap.led_count() as usize].into_boxed_slice();
        Renderer {
            cfg,
            keymap,
            note_color,
            level: [0; 128],
            peak: [0; 128],
            hand_color: [Rgb::new(0, 0, 255), Rgb::new(255, 128, 0)],
            backlight: Rgb::BLACK,
            lit: [false; 128],
            gap: [0; 128],
            feedback: [None; 128],
            clock_us: 0,
            idle_us: 0,
            pixels,
            gamma,
            was_dark: false, // force one blanking frame at start-up
        }
    }

    /// Applies UI settings. The strip length is fixed at start-up, so the
    /// key map is always built for the existing pixel buffer.
    pub fn apply(&mut self, s: &Settings) {
        self.cfg.brightness = s.brightness;
        self.cfg.fade = s.fade();
        self.cfg.velocity = s.velocity;
        self.cfg.max_milliamps = s.max_ma;
        self.gamma = GammaLut::new(s.brightness, self.cfg.gamma);
        self.keymap = KeyMap::new(&s.geometry(self.pixels.len() as u16));
        self.note_color = s.color_mode().note_table();
        self.hand_color = s.hand_colors();
        self.backlight = s.backlight();
    }

    /// Advances envelopes by `dt` and rebuilds the pixel buffer. Returns
    /// `false` when the frame is dark and the previous one was too, so the
    /// caller can skip the SPI transfer entirely while idle.
    pub fn update(&mut self, notes: &NoteState, dt: Duration) -> bool {
        let dt_us = dt.as_micros().min(u32::MAX as u128) as u64;
        let decay = |fade: Duration| -> u32 {
            let f = fade.as_micros().max(1) as u64;
            ((MAX_LEVEL as u64 * dt_us) / f).min(MAX_LEVEL as u64) as u32
        };
        let release = decay(self.cfg.fade);

        // Notes first on black; the backlight is merged in afterwards (per-channel
        // max, so the result is the same) once we know whether it's asleep.
        self.pixels.fill(Rgb::BLACK);
        let mut any = false;
        let mut active = notes.playing();
        // Hints must stand out against the backlight, whatever the brightness.
        let bl = self.backlight;
        let hint_floor = bl.r.max(bl.g).max(bl.b).saturating_mul(2);
        self.clock_us = self.clock_us.wrapping_add(dt_us);
        let blink_off = (self.clock_us / BLINK_HALF_US) % 2 == 1;
        let practice = notes.practice();

        for n in 0..128usize {
            let struck = notes.take_struck(n);
            active |= struck != 0 || notes.held(n) != 0;
            self.gap[n] = self.gap[n].saturating_sub(dt_us.min(u32::MAX as u64) as u32);
            if (struck != 0 || notes.take_hint_struck(n)) && self.lit[n] {
                self.gap[n] = RESTRIKE_GAP_US;
            }
            if struck != 0 {
                // Judge the press against what the player expects right now.
                let h = notes.hint(n);
                // Previews (high nibble) are not "expected yet".
                let expected = h & 3 != 0 && h & HINT_BRIGHT == 0 && h >> HINT_PREVIEW_SHIFT == 0;
                self.feedback[n] = practice.then(|| if expected { RIGHT_KEY } else { WRONG_KEY });
                self.peak[n] = if self.cfg.velocity {
                    // Velocity curve: floor at ~25% so soft notes stay visible.
                    (64 + (struck as u32 * 191) / 127) << 8
                } else {
                    MAX_LEVEL
                };
                self.level[n] = self.peak[n];
            } else if notes.held(n) != 0 {
                self.level[n] = self.peak[n];
            } else if self.level[n] != 0 {
                self.level[n] = self.level[n].saturating_sub(release);
            }

            let lvl = (self.level[n] >> 8) as u8;
            let h = notes.hint(n);
            let c = if lvl != 0 {
                let base = self.feedback[n].unwrap_or(self.note_color[n]);
                self.gamma.apply(base.scale(lvl))
            } else if h & HINT_BLINK != 0 && blink_off {
                Rgb::BLACK
            } else if h != 0 {
                // Keys you play win; otherwise show the player's hint.
                let hand = self.hand_color[((h & 3) as usize).saturating_sub(1).min(1)];
                let preview = (h >> HINT_PREVIEW_SHIFT) as u32;
                if preview != 0 {
                    // Upcoming note: faint, a little above the backlight so it shows.
                    let l = PREVIEW_MIN + (PREVIEW_MAX - PREVIEW_MIN) * (preview - 1) / 14;
                    let c = self.gamma.apply(hand.scale(l as u8));
                    let floor = (hint_floor / 2).saturating_add(2);
                    if c.r.max(c.g).max(c.b) < floor { hand.scale(floor) } else { c }
                } else {
                    let l = if h & HINT_BRIGHT != 0 { HINT_AUTO } else { 255 };
                    let c = self.gamma.apply(hand.scale(l));
                    if c.r.max(c.g).max(c.b) < hint_floor { hand.scale(hint_floor) } else { c }
                }
            } else {
                Rgb::BLACK
            };
            if c.is_black() {
                self.lit[n] = false;
                self.gap[n] = 0;
                continue;
            }
            self.lit[n] = true;
            let c = if self.gap[n] > 0 { c.scale(RESTRIKE_DIM) } else { c };
            any = true;
            for px in &mut self.pixels[self.keymap.leds_for(n as u8)] {
                *px = px.max(c);
            }
        }

        self.idle_us = if active { 0 } else { self.idle_us.saturating_add(dt_us) };
        if !self.backlight.is_black() && self.idle_us < SCREENSAVER_US {
            any = true; // a lit backlight means every frame has to go out
            let bl = self.backlight;
            for px in self.pixels.iter_mut() {
                *px = px.max(bl);
            }
        }

        if any && self.cfg.max_milliamps > 0 {
            self.limit_power();
        }

        let send = any || !self.was_dark;
        self.was_dark = !any;
        send
    }

    /// Scales the whole frame down uniformly if it would exceed the budget.
    fn limit_power(&mut self) {
        let led_count = self.pixels.len() as u32;
        let idle = led_count; // ~1 mA per LED
        let budget = self.cfg.max_milliamps.saturating_sub(idle).max(1);
        let sum: u32 = self
            .pixels
            .iter()
            .map(|p| p.r as u32 + p.g as u32 + p.b as u32)
            .sum();
        // 20 mA per channel at 255.
        let draw = sum * 20 / 255;
        if draw <= budget {
            return;
        }
        let k = ((budget * 255) / draw).min(255) as u8;
        for p in self.pixels.iter_mut() {
            *p = p.scale(k);
        }
    }

    pub fn pixels(&self) -> &[Rgb] {
        &self.pixels
    }
}

/// Render loop. Runs at `fps`, but a note-on unparks the thread so a strike
/// is drawn immediately instead of waiting up to a full frame period.
pub fn run(
    mut renderer: Renderer,
    mut driver: Box<dyn LedDriver>,
    notes: Arc<NoteState>,
    settings: Arc<SettingsStore>,
    running: Arc<AtomicBool>,
) {
    let mut seen_gen = u32::MAX;
    let period = Duration::from_micros(1_000_000 / renderer.cfg.fps.max(1) as u64);
    // Upper bound on how often an early wake may trigger a frame, so a
    // glissando can't saturate the SPI bus.
    let min_gap = Duration::from_millis(2);
    let mut last = Instant::now();
    let mut next = last + period;
    let mut errors = 0u32;

    while running.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now < next {
            std::thread::park_timeout(next - now);
            // Woken early by a strike (or spuriously): render now, but never
            // closer than `min_gap` to the previous frame.
            let since = last.elapsed();
            if since < min_gap {
                std::thread::sleep(min_gap - since);
            }
        }
        let gen = settings.generation();
        if gen != seen_gen {
            // try_get: if the web thread holds the lock, just retry next frame.
            if let Some(s) = settings.try_get() {
                renderer.apply(&s);
                seen_gen = gen;
            }
        }

        let now = Instant::now();
        let dt = now - last;
        last = now;
        next = now + period;

        if renderer.update(&notes, dt) {
            if let Err(e) = driver.show(renderer.pixels()) {
                errors += 1;
                // Log sparingly; a disconnected strip shouldn't flood journald.
                if errors.is_power_of_two() {
                    eprintln!("led driver error (#{errors}): {e}");
                }
            }
        }
    }
    // Driver's Drop blanks the strip.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::StripGeometry;

    fn renderer() -> Renderer {
        let cfg = RenderConfig { brightness: 255, gamma: 1.0, velocity: false, ..Default::default() };
        Renderer::new(cfg, KeyMap::new(&StripGeometry::default()), [Rgb::new(255, 0, 0); 128])
    }

    #[test]
    fn strike_lights_and_fades() {
        let mut r = renderer();
        let notes = NoteState::new();
        assert!(r.update(&notes, Duration::ZERO)); // initial blanking frame
        assert!(!r.update(&notes, Duration::ZERO)); // idle -> skip

        notes.note_on(60, 127);
        notes.note_off(60); // tapped between frames
        assert!(r.update(&notes, Duration::from_millis(1)));
        assert_eq!(r.pixels()[80], Rgb::new(255, 0, 0));

        // Half the fade time later, roughly half brightness.
        r.update(&notes, Duration::from_millis(175));
        let red = r.pixels()[80].r;
        assert!((120..=135).contains(&red), "{red}");

        r.update(&notes, Duration::from_millis(500));
        assert!(r.pixels()[80].is_black());
    }

    #[test]
    fn held_note_stays_lit() {
        let mut r = renderer();
        let notes = NoteState::new();
        notes.note_on(21, 64);
        for _ in 0..10 {
            r.update(&notes, Duration::from_millis(100));
        }
        assert_eq!(r.pixels()[2], Rgb::new(255, 0, 0));
    }

    #[test]
    fn hints_draw_under_live_notes() {
        let mut r = renderer();
        r.hand_color = [Rgb::new(0, 0, 255), Rgb::new(0, 255, 0)];
        let notes = NoteState::new();
        notes.set_hint(21, crate::notes::HINT_LEFT);
        notes.set_hint(60, crate::notes::HINT_RIGHT | HINT_BRIGHT);
        r.update(&notes, Duration::ZERO);
        // Keys to play are full brightness; notes the player sounds a bit less.
        assert_eq!(r.pixels()[2], Rgb::new(0, 255, 0));
        assert_eq!(r.pixels()[80], Rgb::new(0, 0, HINT_AUTO));
        // Playing the hinted key shows the live colour (after the re-strike dip).
        notes.note_on(60, 127);
        r.update(&notes, Duration::from_millis(50));
        r.update(&notes, Duration::from_millis(50));
        assert_eq!(r.pixels()[80], Rgb::new(255, 0, 0));
    }

    #[test]
    fn restrike_of_a_lit_key_dips_then_repeaks() {
        let mut r = renderer();
        let notes = NoteState::new();
        notes.note_on(60, 127);
        r.update(&notes, Duration::from_millis(16));
        assert_eq!(r.pixels()[80], Rgb::new(255, 0, 0), "first strike: no dip");
        // Same key again while still held (no note-off in between).
        notes.note_on(60, 127);
        r.update(&notes, Duration::from_millis(2));
        let dipped = r.pixels()[80].r;
        assert!(dipped < 100, "{dipped}");
        r.update(&notes, Duration::from_millis(16));
        assert!(r.pixels()[80].r < 100, "dip lasts a few frames");
        r.update(&notes, Duration::from_millis(30));
        assert_eq!(r.pixels()[80], Rgb::new(255, 0, 0), "back to full");
        // Player re-strikes on a hint behave the same.
        notes.note_off(60);
        r.update(&notes, Duration::from_secs(1));
        notes.set_hint(60, crate::notes::HINT_RIGHT | HINT_BRIGHT);
        r.update(&notes, Duration::from_millis(16));
        notes.strike_hint(60);
        r.update(&notes, Duration::from_millis(2));
        assert!(r.pixels()[80].b < HINT_AUTO / 2);
    }

    #[test]
    fn practice_presses_show_green_or_red() {
        let mut r = renderer();
        r.note_color = [Rgb::new(0, 0, 255); 128]; // normal key colour: blue
        let notes = NoteState::new();
        notes.set_practice(true);
        notes.set_hint(60, crate::notes::HINT_RIGHT);
        notes.note_on(60, 127); // expected key
        notes.note_on(62, 127); // wrong key
        r.update(&notes, Duration::from_millis(50));
        r.update(&notes, Duration::from_millis(50)); // past the re-strike dip over the hint
        assert_eq!(r.pixels()[80], RIGHT_KEY);
        assert_eq!(r.pixels()[84], WRONG_KEY);
        // Outside practice the normal key colour is used.
        notes.set_practice(false);
        notes.note_on(64, 127);
        r.update(&notes, Duration::from_millis(16));
        assert_eq!(r.pixels()[88], Rgb::new(0, 0, 255));
    }

    #[test]
    fn blinking_hint_toggles() {
        let mut r = renderer();
        let notes = NoteState::new();
        notes.set_hint(60, crate::notes::HINT_RIGHT | HINT_BLINK);
        let mut seen = (false, false);
        for _ in 0..16 {
            r.update(&notes, Duration::from_millis(16));
            if r.pixels()[80].is_black() { seen.0 = true } else { seen.1 = true }
        }
        assert_eq!(seen, (true, true), "blinks on and off");
    }

    #[test]
    fn backlight_sleeps_after_two_idle_minutes() {
        let mut r = renderer();
        r.apply(&Settings { backlight_color: "#00ffff".into(), backlight_brightness: 15, ..Settings::default() });
        let notes = NoteState::new();
        r.update(&notes, Duration::from_secs(119));
        assert_eq!(r.pixels()[0], Rgb::new(0, 15, 15), "still on");
        r.update(&notes, Duration::from_secs(2));
        assert!(r.pixels()[0].is_black(), "asleep after 2 minutes");
        assert!(!r.update(&notes, Duration::from_millis(16)), "dark frames are skipped");
        notes.note_on(60, 100);
        r.update(&notes, Duration::from_millis(16));
        assert_eq!(r.pixels()[0], Rgb::new(0, 15, 15), "a key press wakes it at once");
        // A playing song keeps it awake.
        notes.note_off(60);
        notes.set_playing(true);
        r.update(&notes, Duration::from_secs(300));
        assert_eq!(r.pixels()[0], Rgb::new(0, 15, 15));
    }

    #[test]
    fn previews_are_faint_and_never_judged_right() {
        let mut r = renderer();
        r.hand_color = [Rgb::new(0, 0, 255), Rgb::new(0, 255, 0)];
        let notes = NoteState::new();
        notes.set_hint(60, crate::notes::HINT_RIGHT | 15 << HINT_PREVIEW_SHIFT);
        notes.set_hint(62, crate::notes::HINT_RIGHT | 1 << HINT_PREVIEW_SHIFT);
        r.update(&notes, Duration::ZERO);
        let (near, far) = (r.pixels()[80].b, r.pixels()[84].b);
        assert!(far > 0 && far < near && near < 100, "faint, brighter when closer: {far} {near}");
        // Pressing a key that's only previewed counts as wrong (red).
        notes.set_practice(true);
        notes.note_on(60, 127);
        r.update(&notes, Duration::from_millis(50));
        r.update(&notes, Duration::from_millis(50));
        assert_eq!(r.pixels()[80], WRONG_KEY);
    }

    #[test]
    fn hints_stay_brighter_than_the_backlight() {
        let mut r = renderer();
        let s = Settings { backlight_color: "#00ffff".into(), backlight_brightness: 15, brightness: 10, ..Settings::default() };
        r.apply(&s);
        let notes = NoteState::new();
        notes.set_hint(60, crate::notes::HINT_RIGHT);
        r.update(&notes, Duration::ZERO);
        let led = KeyMap::new(&s.geometry(176)).led_for(60).unwrap() as usize;
        let p = r.pixels()[led];
        assert!(p.r.max(p.g).max(p.b) >= 30, "{p:?}");
    }

    #[test]
    fn backlight_fills_strip_under_notes() {
        let mut r = renderer();
        let s = Settings { backlight_color: "#00ffff".into(), backlight_brightness: 15, ..Settings::default() };
        r.apply(&Settings { brightness: 255, color: "#ff0000".into(), ..s });
        let notes = NoteState::new();
        assert!(r.update(&notes, Duration::ZERO));
        assert!(r.update(&notes, Duration::ZERO), "lit backlight must keep sending");
        assert!(r.pixels().iter().all(|&p| p == Rgb::new(0, 15, 15)));
        notes.note_on(60, 127);
        r.update(&notes, Duration::ZERO);
        let key = r.pixels()[KeyMap::new(&Settings::default().geometry(176)).led_for(60).unwrap() as usize];
        assert_eq!(key, Rgb::new(255, 15, 15));
    }

    #[test]
    fn power_limit_caps_draw() {
        let cfg = RenderConfig { brightness: 255, gamma: 1.0, velocity: false, max_milliamps: 300, ..Default::default() };
        let mut r = Renderer::new(cfg, KeyMap::new(&StripGeometry::default()), [Rgb::new(255, 255, 255); 128]);
        let notes = NoteState::new();
        for n in 21..108 {
            notes.note_on(n, 127);
        }
        r.update(&notes, Duration::ZERO);
        let sum: u32 = r.pixels().iter().map(|p| p.r as u32 + p.g as u32 + p.b as u32).sum();
        assert!(sum * 20 / 255 <= 300 - 176);
    }
}
