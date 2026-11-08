//! Command-line configuration. Hand-rolled to keep the dependency tree (and
//! binary) small; every option maps 1:1 to a field below.

use std::path::PathBuf;
use std::time::Duration;

use crate::color::Rgb;
use crate::hal::{DriverKind, HwConfig};
use crate::keymap::StripGeometry;
use crate::render::RenderConfig;

#[derive(Clone, Debug)]
pub enum ColorMode {
    Single(Rgb),
    /// Hue sweeps across the 88 keys (A0..C8): `circles` colour circles from
    /// one end to the other, drifting `speed` degrees per second (0 = still).
    Rainbow { circles: f32, speed: f32 },
    /// One colour per pitch class, the same in every octave: C magenta
    /// through blue, green and yellow to B red (the "musical spiral").
    Spiral,
    /// From the lowest key's colour to the highest key's.
    Gradient(Rgb, Rgb),
    /// Notes of the major scale on `key` (0 = C) in one colour, the rest in
    /// another: in C that is white keys against black keys.
    Scale { key: u8, inside: Rgb, outside: Rgb },
    /// Colours for key ranges (low, high, colour); keys between ranges blend
    /// from the neighbouring ranges' colours.
    Multi(Vec<(u8, u8, Rgb)>),
    /// The colour follows how hard the key is struck: soft blue to hard red.
    Velocity,
}

/// Spiral colours for C, C♯ … B.
const SPIRAL: [Rgb; 12] = [
    Rgb::new(255, 0, 200), // C magenta
    Rgb::new(190, 0, 255), // C♯ purple
    Rgb::new(110, 0, 255), // D violet
    Rgb::new(0, 40, 255),  // D♯ blue
    Rgb::new(0, 200, 255), // E cyan
    Rgb::new(0, 255, 120), // F teal
    Rgb::new(80, 255, 0),  // F♯ green
    Rgb::new(255, 230, 0), // G yellow
    Rgb::new(255, 170, 0), // G♯ amber
    Rgb::new(255, 110, 0), // A orange
    Rgb::new(255, 50, 0),  // A♯ red-orange
    Rgb::new(255, 0, 0),   // B red
];

impl ColorMode {
    pub const RAINBOW: ColorMode = ColorMode::Rainbow { circles: 1.0, speed: 0.0 };

    /// `rainbow`, `spiral` or `RRGGBB` / `#RRGGBB`.
    pub fn parse(s: &str) -> Option<Self> {
        if s == "rainbow" {
            Some(ColorMode::RAINBOW)
        } else if s == "spiral" {
            Some(ColorMode::Spiral)
        } else {
            Rgb::from_hex(s).map(ColorMode::Single)
        }
    }

    /// Base colour per MIDI note at `secs` seconds (only a moving rainbow
    /// changes with time). Allocation-free, so the render loop may call it.
    pub fn note_table(&self, secs: f32) -> [Rgb; 128] {
        let mut t = [Rgb::BLACK; 128];
        for (n, c) in t.iter_mut().enumerate() {
            // Position across the 88 keys, 0 (A0) to 1 (C8).
            let k = (n as f32 - 21.0).clamp(0.0, 87.0) / 87.0;
            *c = match self {
                ColorMode::Single(c) => *c,
                ColorMode::Rainbow { circles, speed } => {
                    Rgb::from_hsv((k * 360.0 * circles + secs * speed).rem_euclid(360.0), 1.0, 1.0)
                }
                ColorMode::Spiral => SPIRAL[n % 12],
                ColorMode::Gradient(a, b) => mix(*a, *b, k),
                ColorMode::Scale { key, inside, outside } => {
                    if MAJOR.contains(&((n as u8 + 12 - key % 12) % 12)) { *inside } else { *outside }
                }
                ColorMode::Multi(ranges) => multi_color(ranges, n as u8),
                ColorMode::Velocity => velocity_color(64), // each strike picks its own colour
            };
        }
        t
    }
}

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

/// Straight blend from `a` (t = 0) to `b` (t = 1).
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// Velocity Rainbow: soft (1) blue through green and yellow to hard (127) red.
pub fn velocity_color(vel: u8) -> Rgb {
    Rgb::from_hsv(240.0 * (1.0 - vel.min(127) as f32 / 127.0), 1.0, 1.0)
}

/// Multicolor: a key inside a range takes its colour; one between ranges
/// blends from the nearest range on each side; past the last range on a side
/// it keeps that range's colour.
fn multi_color(ranges: &[(u8, u8, Rgb)], n: u8) -> Rgb {
    if let Some(&(_, _, c)) = ranges.iter().find(|r| (r.0..=r.1).contains(&n)) {
        return c;
    }
    let below = ranges.iter().filter(|r| r.1 < n).max_by_key(|r| r.1);
    let above = ranges.iter().filter(|r| r.0 > n).min_by_key(|r| r.0);
    match (below, above) {
        (Some(l), Some(h)) => mix(l.2, h.2, (n - l.1) as f32 / (h.0 - l.1) as f32),
        (Some(r), None) | (None, Some(r)) => r.2,
        (None, None) => Rgb::new(255, 255, 255),
    }
}

#[cfg(test)]
mod color_mode_tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 255);

    #[test]
    fn gradient_runs_from_the_lowest_to_the_highest_key() {
        let t = ColorMode::Gradient(RED, BLUE).note_table(0.0);
        assert_eq!((t[21], t[108]), (RED, BLUE));
        assert!(t[64].r > 0 && t[64].b > 0, "a blend in the middle: {:?}", t[64]);
    }

    #[test]
    fn scale_in_c_splits_white_and_black_keys() {
        let t = ColorMode::Scale { key: 0, inside: RED, outside: BLUE }.note_table(0.0);
        assert_eq!((t[60], t[62], t[61], t[66]), (RED, RED, BLUE, BLUE), "C D white, C♯ F♯ black");
        let g = ColorMode::Scale { key: 7, inside: RED, outside: BLUE }.note_table(0.0);
        assert_eq!((g[66], g[65]), (RED, BLUE), "G major: F♯ in, F out");
    }

    #[test]
    fn multicolor_blends_between_ranges() {
        let t = ColorMode::Multi(vec![(21, 40, RED), (61, 108, BLUE)]).note_table(0.0);
        assert_eq!((t[30], t[80]), (RED, BLUE));
        let mid = t[50]; // about halfway between 40 and 61
        assert!(mid.r > 100 && mid.b > 100, "blended: {mid:?}");
        let one = ColorMode::Multi(vec![(60, 70, BLUE)]).note_table(0.0);
        assert_eq!((one[21], one[108]), (BLUE, BLUE), "one range colours the rest too");
    }

    #[test]
    fn a_moving_rainbow_drifts_with_time() {
        let still = ColorMode::RAINBOW;
        assert_eq!(still.note_table(0.0), still.note_table(10.0));
        let moving = ColorMode::Rainbow { circles: 1.0, speed: 36.0 };
        assert_ne!(moving.note_table(0.0)[60], moving.note_table(1.0)[60]);
        // After 10 s at 36°/s it has gone round exactly once.
        assert_eq!(moving.note_table(0.0), moving.note_table(10.0));
        // Two circles: the colour at the middle key comes back to the start colour.
        let two = ColorMode::Rainbow { circles: 2.0, speed: 0.0 }.note_table(0.0);
        assert_eq!(two[21], Rgb::new(255, 0, 0));
    }

    #[test]
    fn velocity_goes_from_blue_to_red() {
        assert_eq!(velocity_color(1).b, 255);
        assert_eq!(velocity_color(127), RED);
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub geometry: StripGeometry,
    pub render: RenderConfig,
    pub color: ColorMode,
    pub driver: DriverKind,
    pub hw: HwConfig,
    pub midi_port: Option<String>,
    pub list_ports: bool,
    pub demo: bool,
    /// Web UI listen address; `None` disables it.
    pub http: Option<String>,
    pub songs_dir: PathBuf,
    pub settings_path: Option<PathBuf>,
    /// HTTPS listen address (needed to install the app on Android); `None` disables it.
    pub https: Option<String>,
    /// RTP-MIDI (network MIDI) session port; data uses the next one. `None` = off.
    pub rtpmidi: Option<u16>,
    /// Tablet view: `waterfall` (falling notes) or `sheet` (staff notation).
    pub view_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            geometry: StripGeometry::default(),
            render: RenderConfig::default(),
            color: ColorMode::RAINBOW,
            driver: if !cfg!(target_os = "linux") {
                DriverKind::Terminal
            } else if cfg!(feature = "pwm") {
                DriverKind::Pwm
            } else {
                DriverKind::Spi
            },
            hw: HwConfig { spi_bus: 0, pwm_gpio: 18, dma_channel: 10 },
            midi_port: None,
            list_ports: false,
            demo: false,
            http: Some("0.0.0.0:8080".into()),
            https: Some("0.0.0.0:8443".into()),
            rtpmidi: Some(5004),
            songs_dir: PathBuf::from("songs"),
            settings_path: Some(PathBuf::from("settings.json")),
            view_mode: "waterfall".into(),
        }
    }
}

pub const VIEW_MODES: [&str; 2] = ["waterfall", "sheet"];

pub const USAGE: &str = "\
plv - Piano LED Visualizer engine (WS2812B over SPI)

USAGE: plv [OPTIONS]

Strip:
  --leds <N>              LED count                          [176]
  --leds-per-meter <N>    strip density (72 = 1 LED/semitone) [144]
  --shift <N>             global shift in LEDs                [0]
  --reverse               LED 0 is at the treble end
  --offset <NOTE:LEDS>    cumulative offset for notes above NOTE (repeatable)
  --span <N>              LEDs lit per key                    [1]

Look:
  --color <rainbow|spiral|RRGGBB>                             [rainbow]
  --brightness <0-255>                                        [128]
  --gamma <F>                                                 [2.2]
  --fade-ms <MS>          release fade                        [350]
  --no-velocity           ignore velocity for brightness
  --max-ma <MA>           current limit for the strip, 0=off  [0]
  --fps <N>               frame rate                          [60]

I/O:
  --driver <pwm|spi|terminal|null>     [pwm if built with it, else spi]
  --gpio <N>              PWM data pin (12/18 = PWM0, 13/19 = PWM1) [18]
  --dma <N>               DMA channel for PWM (never 5 on Zero 2 W) [10]
  --spi-bus <0|1>         SPI data on GPIO10 (bus 0) / GPIO20 (bus 1) [0]
  --port <SUBSTR>         MIDI input port name filter         [first device]
  --list-ports            print MIDI inputs and exit
  --demo                  play a scale without a MIDI device

App:
  --http <ADDR|off>       web UI address                      [0.0.0.0:8080]
  --https <ADDR|off>      web UI over HTTPS (installable app) [0.0.0.0:8443]
  --rtpmidi <PORT|off>    network MIDI (RTP-MIDI) session port [5004]
  --songs <DIR>           folder with .mid files              [songs]
  --settings <FILE|off>   saved UI settings (override flags)  [settings.json]
  --view <waterfall|sheet>  tablet view on first start         [waterfall]
  -h, --help
";

pub fn parse<I: Iterator<Item = String>>(mut args: I) -> Result<Config, String> {
    let mut c = Config::default();
    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--leds" => c.geometry.led_count = num(&val(&a)?, &a)?,
            "--leds-per-meter" => c.geometry.leds_per_meter = num(&val(&a)?, &a)?,
            "--shift" => c.geometry.shift = num(&val(&a)?, &a)?,
            "--reverse" => c.geometry.reverse = true,
            "--span" => c.geometry.span = num(&val(&a)?, &a)?,
            "--offset" => {
                let v = val(&a)?;
                let (n, o) = v.split_once(':').ok_or("--offset expects NOTE:LEDS")?;
                c.geometry.note_offsets.push((num(n, &a)?, num(o, &a)?));
            }
            "--color" => {
                let v = val(&a)?;
                c.color = ColorMode::parse(&v).ok_or("--color expects rainbow, spiral or RRGGBB")?;
            }
            "--brightness" => c.render.brightness = num(&val(&a)?, &a)?,
            "--gamma" => c.render.gamma = num(&val(&a)?, &a)?,
            "--fade-ms" => c.render.fade = Duration::from_millis(num(&val(&a)?, &a)?),
            "--no-velocity" => c.render.velocity = false,
            "--max-ma" => c.render.max_milliamps = num(&val(&a)?, &a)?,
            "--fps" => c.render.fps = num(&val(&a)?, &a)?,
            "--driver" => {
                let v = val(&a)?;
                c.driver = DriverKind::parse(&v).ok_or_else(|| format!("unknown driver '{v}'"))?;
            }
            "--spi-bus" => c.hw.spi_bus = num(&val(&a)?, &a)?,
            "--gpio" => c.hw.pwm_gpio = num(&val(&a)?, &a)?,
            "--dma" => c.hw.dma_channel = num(&val(&a)?, &a)?,
            "--port" => c.midi_port = Some(val(&a)?),
            "--list-ports" => c.list_ports = true,
            "--demo" => c.demo = true,
            "--http" => c.http = Some(val(&a)?).filter(|v| v != "off"),
            "--https" => c.https = Some(val(&a)?).filter(|v| v != "off"),
            "--rtpmidi" => {
                let v = val(&a)?;
                c.rtpmidi = if v == "off" { None } else { Some(num::<u16>(&v, &a)?).filter(|&p| p < u16::MAX) };
            }
            "--songs" => c.songs_dir = PathBuf::from(val(&a)?),
            "--view" => {
                let v = val(&a)?;
                if !VIEW_MODES.contains(&v.as_str()) {
                    return Err("--view expects waterfall or sheet".into());
                }
                c.view_mode = v;
            }
            "--settings" => c.settings_path = Some(val(&a)?).filter(|v| v != "off").map(PathBuf::from),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    if c.geometry.led_count == 0 {
        return Err("--leds must be > 0".into());
    }
    if !matches!(c.hw.pwm_gpio, 12 | 13 | 18 | 19) {
        return Err("--gpio must be a PWM pin: 12, 13, 18 or 19".into());
    }
    if matches!(c.hw.dma_channel, 0..=4 | 5 | 15..) {
        return Err("--dma must be 6..=14 (5 is the SD card, 0-4 are in use by the firmware)".into());
    }
    if !(1..=400).contains(&c.render.fps) {
        return Err("--fps must be 1..=400".into());
    }
    Ok(c)
}

fn num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("invalid value '{s}' for {flag}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> impl Iterator<Item = String> + '_ {
        s.split_whitespace().map(String::from)
    }

    #[test]
    fn parses_flags() {
        let c = parse(args("--leds 200 --reverse --offset 60:1 --offset 90:-2 --color ff0000 --driver null --gpio 12 --dma 11")).unwrap();
        assert_eq!((c.hw.pwm_gpio, c.hw.dma_channel), (12, 11));
        assert_eq!(c.geometry.led_count, 200);
        assert!(c.geometry.reverse);
        assert_eq!(c.geometry.note_offsets, vec![(60, 1), (90, -2)]);
        assert_eq!(c.driver, DriverKind::Null);
        assert!(matches!(c.color, ColorMode::Single(Rgb { r: 255, g: 0, b: 0 })));
        let t = ColorMode::parse("spiral").unwrap().note_table(0.0);
        assert_eq!((t[60], t[72], t[71]), (Rgb::new(255, 0, 200), Rgb::new(255, 0, 200), Rgb::new(255, 0, 0)), "C pink in every octave, B red");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(args("--leds")).is_err());
        assert!(parse(args("--leds x")).is_err());
        assert!(parse(args("--bogus")).is_err());
        assert!(parse(args("--offset 60")).is_err());
        assert!(parse(args("--gpio 17")).is_err());
        assert!(parse(args("--dma 5")).is_err());
        assert!(parse(args("--view piano")).is_err());
        assert_eq!(parse(args("--view sheet")).unwrap().view_mode, "sheet");
    }
}
