//! Renders the strip as a row of truecolour blocks in the terminal, so the
//! engine can be developed and tested on a desktop with a USB MIDI keyboard.

use std::io::Write;

use crate::color::Rgb;
use crate::hal::LedDriver;

const MAX_COLS: usize = 100;

pub struct TerminalDriver {
    line: String,
    leds_per_col: usize,
}

impl TerminalDriver {
    pub fn new(led_count: u16) -> Self {
        let leds_per_col = (led_count as usize).div_ceil(MAX_COLS).max(1);
        let cols = (led_count as usize).div_ceil(leds_per_col);
        TerminalDriver {
            // ~20 bytes of escape code per column; reserved once, reused forever.
            line: String::with_capacity(cols * 24 + 16),
            leds_per_col,
        }
    }
}

impl LedDriver for TerminalDriver {
    fn show(&mut self, pixels: &[Rgb]) -> Result<(), String> {
        use std::fmt::Write as _;
        self.line.clear();
        self.line.push('\r');
        for group in pixels.chunks(self.leds_per_col) {
            let c = group.iter().fold(Rgb::BLACK, |a, &b| a.max(b));
            // Lift very dim LEDs so they're visible on a terminal.
            let lift = |v: u8| if v == 0 { 24 } else { v.saturating_add(40) };
            let _ = write!(self.line, "\x1b[38;2;{};{};{}m█", lift(c.r), lift(c.g), lift(c.b));
        }
        self.line.push_str("\x1b[0m");
        let mut out = std::io::stdout().lock();
        out.write_all(self.line.as_bytes())
            .and_then(|_| out.flush())
            .map_err(|e| e.to_string())
    }
}
