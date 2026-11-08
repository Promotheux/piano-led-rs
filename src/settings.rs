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

/// Named colour modes for `color` (besides a plain `#rrggbb`).
const COLOR_MODES: [&str; 6] = ["rainbow", "spiral", "gradient", "scale", "multicolor", "velocity"];

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
    /// Colour of keys you play: `rainbow`, `spiral`, `gradient`, `scale`,
    /// `multicolor`, `velocity` or `#rrggbb`; the modes' own settings follow.
    pub color: String,
    /// Rainbow: % of one colour circle across the keyboard (100 = once), and
    /// how fast it drifts in degrees per second (0 = still, < 0 = the other way).
    pub rainbow_scale: u16,
    pub rainbow_speed: i16,
    /// Gradient: the lowest and the highest key's colour.
    pub gradient_colors: [String; 2],
    /// Scale: the key (0 = C … 11 = B) of a major scale, and the colours for
    /// notes in it and out of it (in C: white and black keys).
    pub scale_key: u8,
    pub scale_colors: [String; 2],
    /// Multicolor: key ranges (low, high, colour); keys between blend.
    pub multicolor: Vec<(u8, u8, String)>,
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
    /// Improv mode: what lights up, and the lowest key of the improvising
    /// hand (chords are read below it).
    pub improv_lights: crate::improv::Lights,
    pub improv_split: u8,
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
                ColorMode::Single(c) => format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
                ColorMode::Spiral => "spiral".into(),
                _ => "rainbow".into(), // the command line only knows rainbow, spiral and a colour
            },
            rainbow_scale: 100,
            rainbow_speed: 0,
            gradient_colors: ["#2f7cff".into(), "#ff2d75".into()],
            scale_key: 0,
            scale_colors: ["#ffffff".into(), "#ff2020".into()],
            multicolor: vec![(21, 47, "#2f7cff".into()), (48, 71, "#3ddc97".into()), (72, 108, "#ff8a1f".into())],
            right_color: "#2f7cff".into(),
            left_color: "#ff8a1f".into(),
            midi_port: c.midi_port.clone(),
            hand_reach: crate::player::DEFAULT_REACH,
            backlight_color: "#00ffff".into(),
            backlight_brightness: 0,
            view_mode: c.view_mode.clone(),
            note_names: false,
            improv_lights: crate::improv::Lights::default(),
            improv_split: crate::improv::SPLIT,
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
        check(
            COLOR_MODES.contains(&self.color.as_str()) || hex(&self.color),
            "color must be rainbow, spiral, gradient, scale, multicolor, velocity or #rrggbb",
        )?;
        check((10..=1000).contains(&self.rainbow_scale), "rainbow_scale must be 10..=1000 (%)")?;
        check(self.rainbow_speed.abs() <= 360, "rainbow_speed must be -360..=360 (°/s)")?;
        check(self.gradient_colors.iter().all(|c| hex(c)), "gradient_colors must be #rrggbb")?;
        check(self.scale_key < 12, "scale_key must be 0..=11")?;
        check(self.scale_colors.iter().all(|c| hex(c)), "scale_colors must be #rrggbb")?;
        check(self.multicolor.len() <= 16, "at most 16 multicolor ranges")?;
        check(
            self.multicolor.iter().all(|(lo, hi, c)| (21..=108).contains(lo) && (*lo..=108).contains(hi) && hex(c)),
            "multicolor ranges must be low..=high within 21..=108 (A0..C8), each with a #rrggbb colour",
        )?;
        check(hex(&self.right_color), "right_color must be #rrggbb")?;
        check(hex(&self.left_color), "left_color must be #rrggbb")?;
        check(hex(&self.backlight_color), "backlight_color must be #rrggbb")?;
        check(self.hand_reach <= 48, "hand_reach must be 0..=48")?;
        check(VIEW_MODES.contains(&self.view_mode.as_str()), "view_mode must be waterfall or sheet")?;
        check(crate::improv::SPLIT_RANGE.contains(&self.improv_split), "improv_split must be 36..=84 (C2..C6)")?;
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
        let rgb = |s: &str| Rgb::from_hex(s).unwrap_or(Rgb::new(255, 255, 255));
        match self.color.as_str() {
            "rainbow" => ColorMode::Rainbow { circles: self.rainbow_scale as f32 / 100.0, speed: self.rainbow_speed as f32 },
            "gradient" => ColorMode::Gradient(rgb(&self.gradient_colors[0]), rgb(&self.gradient_colors[1])),
            "scale" => ColorMode::Scale {
                key: self.scale_key,
                inside: rgb(&self.scale_colors[0]),
                outside: rgb(&self.scale_colors[1]),
            },
            "multicolor" => ColorMode::Multi(self.multicolor.iter().map(|(lo, hi, c)| (*lo, *hi, rgb(c))).collect()),
            "velocity" => ColorMode::Velocity,
            other => ColorMode::parse(other).unwrap_or(ColorMode::RAINBOW),
        }
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
    fn colour_modes_and_their_settings() {
        let mut s = Settings::default();
        for mode in COLOR_MODES {
            s.color = mode.into();
            assert!(s.validate().is_ok(), "{mode}");
        }
        s.color = "multicolor".into();
        assert!(matches!(s.color_mode(), ColorMode::Multi(r) if r.len() == 3));
        s.multicolor = vec![(60, 50, "#ff0000".into())];
        assert!(s.validate().is_err(), "low above high");
        s.multicolor = vec![(10, 50, "#ff0000".into())];
        assert!(s.validate().is_err(), "below A0");
        s.multicolor = vec![(21, 50, "red".into())];
        assert!(s.validate().is_err(), "not #rrggbb");
        s = Settings { rainbow_speed: 400, ..Settings::default() };
        assert!(s.validate().is_err());
        s = Settings { scale_key: 12, ..Settings::default() };
        assert!(s.validate().is_err());
        s = Settings { color: "rainbow".into(), rainbow_scale: 200, rainbow_speed: -30, ..Settings::default() };
        assert!(matches!(s.color_mode(), ColorMode::Rainbow { circles, speed } if circles == 2.0 && speed == -30.0));
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
