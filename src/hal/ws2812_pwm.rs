//! WS2812B via the PWM peripheral + DMA on GPIO18 (physical pin 12) — the
//! same wiring and the same C library (`rpi_ws281x`) as the original Python
//! project, so existing builds and the Waveshare LCD HAT (which owns SPI0)
//! keep working.
//!
//! Trade-offs versus the SPI driver:
//! * needs root: the library maps `/dev/mem` and allocates DMA memory through
//!   the VideoCore mailbox;
//! * the PWM block is shared with analogue audio, so `dtparam=audio=off`;
//! * `render()` only kicks off the DMA and returns, so the CPU cost per frame
//!   is just the C library's bit expansion into its DMA buffer.
//!
//! Compiled only with `--features pwm` on Linux, because the bindings crate
//! needs a C compiler and libclang (bindgen) at build time.

#[cfg(all(target_os = "linux", feature = "pwm"))]
pub use imp::Ws2812Pwm;

#[cfg(all(target_os = "linux", feature = "pwm"))]
mod imp {
    use crate::color::Rgb;
    use crate::hal::LedDriver;
    use rs_ws281x::{ChannelBuilder, Controller, ControllerBuilder, StripType};

    pub struct Ws2812Pwm {
        ctl: Controller,
    }

    // `Controller` holds raw pointers into C-owned buffers, which makes it
    // !Send. The library has no thread affinity; we own the only handle and
    // move it to the render thread once, so transferring it is sound.
    unsafe impl Send for Ws2812Pwm {}

    impl Ws2812Pwm {
        pub fn open(led_count: u16, gpio: u8, dma: u8) -> Result<Self, String> {
            let ctl = ControllerBuilder::new()
                .freq(800_000)
                // DMA 5 is used by the SD card on Pi 3-class SoCs (incl. Zero 2 W).
                .dma(dma as i32)
                .channel(
                    0,
                    ChannelBuilder::new()
                        .pin(gpio as i32)
                        .count(led_count as i32)
                        // GRB wire order is handled by the library.
                        .strip_type(StripType::Ws2812)
                        // Brightness is already folded into our gamma table.
                        .brightness(255)
                        .build(),
                )
                .build()
                .map_err(|e| {
                    format!("rpi_ws281x init on GPIO{gpio}/DMA{dma}: {e} (run as root; set dtparam=audio=off)")
                })?;
            Ok(Ws2812Pwm { ctl })
        }

        fn fill(&mut self, pixels: &[Rgb]) {
            let leds = self.ctl.leds_mut(0);
            // ws2811_led_t is 0x00RRGGBB as a little-endian u32 -> [B, G, R, W].
            for (dst, p) in leds.iter_mut().zip(pixels) {
                *dst = [p.b, p.g, p.r, 0];
            }
        }
    }

    impl LedDriver for Ws2812Pwm {
        fn show(&mut self, pixels: &[Rgb]) -> Result<(), String> {
            self.fill(pixels);
            self.ctl.render().map_err(|e| format!("rpi_ws281x render: {e}"))
        }
    }

    impl Drop for Ws2812Pwm {
        fn drop(&mut self) {
            // Blank the strip and let the DMA finish before ws2811_fini()
            // (run by Controller's Drop) releases the buffers.
            for led in self.ctl.leds_mut(0) {
                *led = [0; 4];
            }
            let _ = self.ctl.render();
            let _ = self.ctl.wait();
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "pwm")))]
pub struct Ws2812Pwm;

#[cfg(not(all(target_os = "linux", feature = "pwm")))]
impl Ws2812Pwm {
    pub fn open(_led_count: u16, _gpio: u8, _dma: u8) -> Result<Self, String> {
        Err(if cfg!(target_os = "linux") {
            "the PWM driver is not compiled in; rebuild with `--features pwm`".into()
        } else {
            "the PWM driver is only available on Linux; use --driver terminal".into()
        })
    }
}

#[cfg(not(all(target_os = "linux", feature = "pwm")))]
impl crate::hal::LedDriver for Ws2812Pwm {
    fn show(&mut self, _pixels: &[crate::color::Rgb]) -> Result<(), String> {
        Ok(())
    }
}
