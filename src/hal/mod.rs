//! Hardware abstraction for the LED strip.
//!
//! The renderer only knows about [`LedDriver`]; the concrete backend is chosen
//! once at start-up. On the Pi there are two, both DMA-driven so the CPU is
//! idle while the strip is clocked out and there is no bit-banging jitter:
//!
//! * [`ws2812_pwm::Ws2812Pwm`] – PWM on GPIO18, the original project's wiring;
//!   works alongside the Waveshare LCD HAT. Needs root and `--features pwm`.
//! * [`ws2812_spi::Ws2812Spi`] – SPI0 MOSI on GPIO10; no root, no C deps.

pub mod terminal;
pub mod ws2812_pwm;
pub mod ws2812_spi;

use crate::color::Rgb;

pub trait LedDriver: Send {
    /// Pushes a full frame to the strip. `pixels.len()` equals the configured
    /// LED count. Must not allocate.
    fn show(&mut self, pixels: &[Rgb]) -> Result<(), String>;
}

/// Discards frames. Useful for benchmarking the engine without hardware.
pub struct NullDriver;

impl LedDriver for NullDriver {
    fn show(&mut self, _pixels: &[Rgb]) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    Pwm,
    Spi,
    Terminal,
    Null,
}

impl DriverKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pwm" => Some(DriverKind::Pwm),
            "spi" => Some(DriverKind::Spi),
            "terminal" | "term" => Some(DriverKind::Terminal),
            "null" | "none" => Some(DriverKind::Null),
            _ => None,
        }
    }
}

/// Pin/bus selection for the hardware drivers.
#[derive(Clone, Copy, Debug)]
pub struct HwConfig {
    pub spi_bus: u8,
    pub pwm_gpio: u8,
    pub dma_channel: u8,
}

pub fn open(kind: DriverKind, led_count: u16, hw: HwConfig) -> Result<Box<dyn LedDriver>, String> {
    match kind {
        DriverKind::Pwm => Ok(Box::new(ws2812_pwm::Ws2812Pwm::open(led_count, hw.pwm_gpio, hw.dma_channel)?)),
        DriverKind::Spi => Ok(Box::new(ws2812_spi::Ws2812Spi::open(led_count, hw.spi_bus)?)),
        DriverKind::Terminal => Ok(Box::new(terminal::TerminalDriver::new(led_count))),
        DriverKind::Null => Ok(Box::new(NullDriver)),
    }
}
