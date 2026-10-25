//! WS2812B over SPI (MOSI = GPIO10 / physical pin 19 for SPI0).
//!
//! Each WS2812 bit is encoded as three SPI bits clocked at 2.4 MHz
//! (417 ns per SPI bit):
//!
//! ```text
//!   WS "0" -> 100  : 417 ns high, 833 ns low   (spec T0H 400 ±150 ns)
//!   WS "1" -> 110  : 833 ns high, 417 ns low   (spec T1H 800 ±150 ns)
//! ```
//!
//! so one colour byte becomes exactly three SPI bytes and one LED is 9 bytes.
//! A 256-entry table does the expansion, making encoding a straight copy.
//! Trailing zero bytes hold the line low for >280 µs to latch the frame.
//!
//! Why SPI rather than PWM (what `rpi_ws281x` uses on GPIO18): it needs no
//! `/dev/mem` / root, doesn't fight the analogue audio for the PWM block, and
//! the kernel driver already does the DMA for us.

// The encoder is only wired up on Linux; on dev hosts it is exercised by tests.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::color::Rgb;

pub const SPI_HZ: u32 = 2_400_000;
pub const BYTES_PER_LED: usize = 9;
/// 100 bytes × 8 bits / 2.4 MHz ≈ 333 µs of low line (WS2812B-V5 needs 280 µs).
pub const RESET_BYTES: usize = 100;

/// Colour byte -> 3 SPI bytes, MSB first.
pub const ENCODE_LUT: [[u8; 3]; 256] = build_lut();

const fn build_lut() -> [[u8; 3]; 256] {
    let mut lut = [[0u8; 3]; 256];
    let mut v = 0;
    while v < 256 {
        let mut bits: u32 = 0;
        let mut i = 0;
        while i < 8 {
            let one = (v >> (7 - i)) & 1 == 1;
            bits = (bits << 3) | if one { 0b110 } else { 0b100 };
            i += 1;
        }
        lut[v] = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        v += 1;
    }
    lut
}

/// Total SPI transfer size for `led_count` LEDs.
pub const fn frame_len(led_count: usize) -> usize {
    led_count * BYTES_PER_LED + RESET_BYTES
}

/// Encodes `pixels` (GRB wire order) into `out`, which must be
/// `frame_len(pixels.len())` bytes long with the reset tail already zeroed.
#[inline]
pub fn encode(pixels: &[Rgb], out: &mut [u8]) {
    debug_assert!(out.len() >= pixels.len() * BYTES_PER_LED);
    for (px, chunk) in pixels.iter().zip(out.chunks_exact_mut(BYTES_PER_LED)) {
        chunk[0..3].copy_from_slice(&ENCODE_LUT[px.g as usize]);
        chunk[3..6].copy_from_slice(&ENCODE_LUT[px.r as usize]);
        chunk[6..9].copy_from_slice(&ENCODE_LUT[px.b as usize]);
    }
}

#[cfg(target_os = "linux")]
pub use linux::Ws2812Spi;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::hal::LedDriver;
    use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

    pub struct Ws2812Spi {
        spi: Spi,
        tx: Box<[u8]>,
        led_count: usize,
    }

    impl Ws2812Spi {
        pub fn open(led_count: u16, bus: u8) -> Result<Self, String> {
            let led_count = led_count as usize;
            let len = frame_len(led_count);
            check_spidev_bufsiz(len)?;
            let bus = match bus {
                0 => Bus::Spi0,
                1 => Bus::Spi1,
                n => return Err(format!("unsupported SPI bus {n}")),
            };
            let spi = Spi::new(bus, SlaveSelect::Ss0, SPI_HZ, Mode::Mode0).map_err(|e| {
                format!("open SPI: {e} (is `dtparam=spi=on` set and the user in group `spi`?)")
            })?;
            // The only heap allocation the driver ever makes.
            let tx = vec![0u8; len].into_boxed_slice();
            Ok(Ws2812Spi { spi, tx, led_count })
        }
    }

    impl LedDriver for Ws2812Spi {
        fn show(&mut self, pixels: &[Rgb]) -> Result<(), String> {
            let n = pixels.len().min(self.led_count);
            encode(&pixels[..n], &mut self.tx);
            self.spi.write(&self.tx).map(|_| ()).map_err(|e| format!("SPI write: {e}"))
        }
    }

    impl Drop for Ws2812Spi {
        fn drop(&mut self) {
            // Blank the strip on exit instead of leaving the last frame lit.
            let zero = ENCODE_LUT[0];
            for c in self.tx[..self.led_count * BYTES_PER_LED].chunks_exact_mut(3) {
                c.copy_from_slice(&zero);
            }
            let _ = self.spi.write(&self.tx);
        }
    }

    /// spidev rejects single transfers larger than its `bufsiz` (4096 by
    /// default ≈ 444 LEDs). Fail early with an actionable message.
    fn check_spidev_bufsiz(len: usize) -> Result<(), String> {
        let Ok(s) = std::fs::read_to_string("/sys/module/spidev/parameters/bufsiz") else {
            return Ok(());
        };
        match s.trim().parse::<usize>() {
            Ok(max) if len > max => Err(format!(
                "frame needs {len} bytes but spidev.bufsiz is {max}; add \
                 `spidev.bufsiz=32768` to /boot/firmware/cmdline.txt and reboot"
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub struct Ws2812Spi;

#[cfg(not(target_os = "linux"))]
impl Ws2812Spi {
    pub fn open(_led_count: u16, _bus: u8) -> Result<Self, String> {
        Err("the SPI driver is only available on Linux; use --driver terminal".into())
    }
}

#[cfg(not(target_os = "linux"))]
impl crate::hal::LedDriver for Ws2812Spi {
    fn show(&mut self, _pixels: &[Rgb]) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_patterns() {
        // 0x00 -> 100 100 100 100 100 100 100 100
        assert_eq!(ENCODE_LUT[0x00], [0b1001_0010, 0b0100_1001, 0b0010_0100]);
        // 0xFF -> 110 110 110 ...
        assert_eq!(ENCODE_LUT[0xff], [0b1101_1011, 0b0110_1101, 0b1011_0110]);
        // 0x80 -> 110 then seven 100s
        assert_eq!(ENCODE_LUT[0x80], [0b1101_0010, 0b0100_1001, 0b0010_0100]);
    }

    #[test]
    fn lut_decodes_back() {
        for v in 0..=255u8 {
            let [a, b, c] = ENCODE_LUT[v as usize];
            let bits = (a as u32) << 16 | (b as u32) << 8 | c as u32;
            let mut out = 0u8;
            for i in 0..8 {
                let sym = (bits >> (21 - 3 * i)) & 0b111;
                assert!(sym == 0b100 || sym == 0b110, "bad symbol for {v}");
                out = (out << 1) | (sym == 0b110) as u8;
            }
            assert_eq!(out, v);
        }
    }

    #[test]
    fn grb_order_and_tail() {
        let px = [Rgb::new(0xff, 0x00, 0x80)];
        let mut buf = vec![0u8; frame_len(1)];
        encode(&px, &mut buf);
        assert_eq!(&buf[0..3], &ENCODE_LUT[0x00]); // G
        assert_eq!(&buf[3..6], &ENCODE_LUT[0xff]); // R
        assert_eq!(&buf[6..9], &ENCODE_LUT[0x80]); // B
        assert!(buf[9..].iter().all(|&b| b == 0));
    }
}
