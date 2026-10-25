//! User-tunable settings, changeable at runtime from the web UI.
//!
//! Writers (the web thread) replace the whole struct under a mutex and bump a
//! generation counter; the render thread compares the counter each frame and
//! only then `try_lock`s, so the hot path never waits on a lock.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::config::{ColorMode, Config, VIEW_MODES};
use crate::keymap::StripGeometry;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Only applied at start-up: the LED driver sizes its buffers once.
    pub led_count: u16,
    pub leds_per_meter: u16,
    pub shift: i32,
    pub reverse: bool,
    pub note_offsets: Vec<(u8, i32)>,
    pub span: u8,
    pub brightness: u8,
    pub fade_ms: u32,
    pub velocity: bool,
    pub max_ma: u32,
    /// Colour of keys you play: `rainbow` or `#rrggbb`.
    pub color: String,
    /// Player hint colours per hand.
    pub right_color: String,
    pub left_color: String,
    pub midi_port: Option<String>,
    /// Widest stretch of one hand in semitones (player skips wider chords).
    pub hand_reach: u8,
    /// Constant glow under the keys; brightness 0 = off. Not affected by the
    /// global brightness, matching the original.
    pub backlight_color: String,
    pub backlight_brightness: u8,
    /// Tablet view (drawn entirely in the browser): `waterfall` or `sheet`.
    pub view_mode: String,
    /// Show pitch names on the notes in the tablet view.
    pub note_names: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::from_config(&Config::default())
    }
}

impl Settings {
    pub fn from_config(c: &Config) -> Self {
        Settings {
            led_count: c.geometry.led_count,
            leds_per_meter: c.geometry.leds_per_meter,
            shift: c.geometry.shift,
            reverse: c.geometry.reverse,
            note_offsets: c.geometry.note_offsets.clone(),
            span: c.geometry.span,
            brightness: c.render.brightness,
            fade_ms: c.render.fade.as_millis() as u32,
            velocity: c.render.velocity,
            max_ma: c.render.max_milliamps,
            color: match c.color {
                ColorMode::Rainbow => "rainbow".into(),
                ColorMode::Single(c) => format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
            },
            right_color: "#2f7cff".into(),
            left_color: "#ff8a1f".into(),
            midi_port: c.midi_port.clone(),
            hand_reach: crate::player::DEFAULT_REACH,
            backlight_color: "#00ffff".into(),
            backlight_brightness: 0,
            view_mode: c.view_mode.clone(),
            note_names: false,
        }
    }

    /// Rejects values the web UI must not be able to set.
    pub fn validate(&self) -> Result<(), String> {
        let check = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        // Exactly `#rrggbb` from the API (the CLI is more lenient).
        let hex = |s: &str| s.len() == 7 && s.starts_with('#') && Rgb::from_hex(s).is_some();
        check((1..=2000).contains(&self.led_count), "led_count must be 1..=2000")?;
        check((1..=500).contains(&self.leds_per_meter), "leds_per_meter must be 1..=500")?;
        check(self.shift.abs() <= 500, "shift out of range")?;
        check(self.note_offsets.len() <= 32, "too many note offsets")?;
        check(self.note_offsets.iter().all(|&(_, o)| o.abs() <= 100), "note offset out of range")?;
        check((1..=16).contains(&self.span), "span must be 1..=16")?;
        check(self.fade_ms <= 20_000, "fade too long")?;
        check(self.color == "rainbow" || hex(&self.color), "color must be rainbow or #rrggbb")?;
        check(hex(&self.right_color), "right_color must be #rrggbb")?;
        check(hex(&self.left_color), "left_color must be #rrggbb")?;
        check(hex(&self.backlight_color), "backlight_color must be #rrggbb")?;
        check(self.hand_reach <= 48, "hand_reach must be 0..=48")?;
        check(VIEW_MODES.contains(&self.view_mode.as_str()), "view_mode must be waterfall or sheet")?;
        check(self.midi_port.as_ref().map_or(true, |p| p.len() <= 128), "midi_port too long")
    }

    pub fn geometry(&self, led_count: u16) -> StripGeometry {
        StripGeometry {
            led_count,
            leds_per_meter: self.leds_per_meter,
            shift: self.shift,
            reverse: self.reverse,
            note_offsets: self.note_offsets.clone(),
            span: self.span,
        }
    }

    pub fn color_mode(&self) -> ColorMode {
        ColorMode::parse(&self.color).unwrap_or(ColorMode::Rainbow)
    }

    pub fn hand_colors(&self) -> [Rgb; 2] {
        let c = |s: &str| Rgb::from_hex(s).unwrap_or(Rgb::new(255, 255, 255));
        [c(&self.right_color), c(&self.left_color)]
    }

    pub fn backlight(&self) -> Rgb {
        Rgb::from_hex(&self.backlight_color).unwrap_or(Rgb::BLACK).scale(self.backlight_brightness)
    }

    pub fn fade(&self) -> Duration {
        Duration::from_millis(self.fade_ms as u64)
    }

}

pub struct SettingsStore {
    current: Mutex<Settings>,
    generation: AtomicU32,
    path: Option<PathBuf>,
}

impl SettingsStore {
    /// Loads `path` if it exists, else starts from `defaults` and writes them
    /// there, so the flags of the first start become the saved settings.
    pub fn load(path: Option<PathBuf>, defaults: Settings) -> Self {
        let missing = path.as_ref().is_some_and(|p| !p.exists());
        let loaded = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| match serde_json::from_str::<Settings>(&s) {
                Ok(v) if v.validate().is_ok() => Some(v),
                _ => {
                    eprintln!("settings: ignoring invalid settings file");
                    None
                }
            });
        let store = SettingsStore {
            current: Mutex::new(loaded.unwrap_or(defaults)),
            generation: AtomicU32::new(0),
            path,
        };
        if missing {
            if let Err(e) = store.save(&store.get()) {
                eprintln!("settings: {e}");
            }
        }
        store
    }

    fn save(&self, s: &Settings) -> Result<(), String> {
        let Some(p) = &self.path else { return Ok(()) };
        // Write-then-rename so a power cut can't leave a truncated file.
        let tmp = p.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json)
            .and_then(|_| std::fs::rename(&tmp, p))
            .map_err(|e| format!("save settings: {e}"))
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Acquire)
    }

    /// Non-blocking read for the render thread.
    pub fn try_get(&self) -> Option<Settings> {
        self.current.try_lock().ok().map(|s| s.clone())
    }

    pub fn set(&self, new: Settings) -> Result<(), String> {
        new.validate()?;
        self.save(&new)?;
        *self.current.lock().unwrap() = new;
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_defaults() {
        let s = Settings::default();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        // Missing fields fall back to defaults, so old files keep loading.
        let partial: Settings = serde_json::from_str(r#"{"brightness": 10}"#).unwrap();
        assert_eq!(partial.brightness, 10);
        assert_eq!(partial.led_count, s.led_count);
    }

    #[test]
    fn validation() {
        let mut s = Settings::default();
        assert!(s.validate().is_ok());
        s.color = "purple".into();
        assert!(s.validate().is_err());
        s = Settings { view_mode: "sheet".into(), ..Settings::default() };
        assert!(s.validate().is_ok());
        s.view_mode = "tabs".into();
        assert!(s.validate().is_err());
        for bad in ["ff0000", "#ff00", "+fffff", "#gg0000", "#ff0000 "] {
            let s = Settings { right_color: bad.into(), ..Settings::default() };
            assert!(s.validate().is_err(), "{bad:?} must be rejected");
        }
        assert!(Settings { color: "abcdef".into(), ..Settings::default() }.validate().is_err());
        assert!(Settings { color: "#ABCdef".into(), ..Settings::default() }.validate().is_ok());
        s = Settings { led_count: 0, ..Settings::default() };
        assert!(s.validate().is_err());
    }

    #[test]
    fn set_bumps_generation() {
        let store = SettingsStore::load(None, Settings::default());
        let g = store.generation();
        store.set(Settings { brightness: 1, ..Settings::default() }).unwrap();
        assert_eq!(store.generation(), g + 1);
        assert!(store.set(Settings { span: 0, ..Settings::default() }).is_err());
        assert_eq!(store.get().brightness, 1);
    }
}
