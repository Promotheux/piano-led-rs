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
    /// Hue sweeps across the 88 keys (A0..C8).
    Rainbow,
}

impl ColorMode {
    /// `rainbow` or `RRGGBB` / `#RRGGBB`.
    pub fn parse(s: &str) -> Option<Self> {
        if s == "rainbow" {
            Some(ColorMode::Rainbow)
        } else {
            Rgb::from_hex(s).map(ColorMode::Single)
        }
    }

    pub fn note_table(&self) -> [Rgb; 128] {
        let mut t = [Rgb::BLACK; 128];
        for (n, c) in t.iter_mut().enumerate() {
            *c = match self {
                ColorMode::Single(c) => *c,
                ColorMode::Rainbow => {
                    let k = (n as f32 - 21.0).clamp(0.0, 87.0) / 88.0;
                    Rgb::from_hsv(k * 360.0, 1.0, 1.0)
                }
            };
        }
        t
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
    /// Tablet view: `waterfall` (falling notes) or `sheet` (staff notation).
    pub view_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            geometry: StripGeometry::default(),
            render: RenderConfig::default(),
            color: ColorMode::Rainbow,
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
  --color <rainbow|RRGGBB>                                    [rainbow]
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
                c.color = ColorMode::parse(&v).ok_or("--color expects rainbow or RRGGBB")?;
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
