//! Colour primitives and init-time lookup tables.
//!
//! Everything float-based in here runs once at start-up; the per-frame path
//! only does table lookups and integer multiplies.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Parses `RRGGBB` or `#RRGGBB`.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        // from_str_radix alone would also accept a leading '+'.
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    /// Scales every channel by `level / 255`.
    #[inline(always)]
    pub fn scale(self, level: u8) -> Self {
        let l = level as u16 + 1; // +1 so that 255 is an exact identity
        Rgb {
            r: ((self.r as u16 * l) >> 8) as u8,
            g: ((self.g as u16 * l) >> 8) as u8,
            b: ((self.b as u16 * l) >> 8) as u8,
        }
    }

    /// Per-channel maximum, used when two keys overlap on the same LED.
    #[inline(always)]
    pub fn max(self, o: Rgb) -> Self {
        Rgb {
            r: self.r.max(o.r),
            g: self.g.max(o.g),
            b: self.b.max(o.b),
        }
    }

    #[inline(always)]
    pub fn is_black(self) -> bool {
        (self.r | self.g | self.b) == 0
    }

    /// `h` in degrees [0, 360), `s`/`v` in [0, 1].
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let c = v * s;
        let hp = (h % 360.0) / 60.0;
        let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb::new(q(r), q(g), q(b))
    }
}

/// Combined global-brightness + gamma correction table, built once.
///
/// WS2812B output is linear in PWM duty, which looks very non-linear to the
/// eye; without gamma correction fades appear to "snap" off at the end.
pub struct GammaLut([u8; 256]);

impl GammaLut {
    pub fn new(brightness: u8, gamma: f32) -> Self {
        let mut t = [0u8; 256];
        let b = brightness as f32 / 255.0;
        for (i, out) in t.iter_mut().enumerate() {
            let lin = (i as f32 / 255.0).powf(gamma) * b;
            *out = (lin * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        GammaLut(t)
    }

    #[inline(always)]
    pub fn apply(&self, c: Rgb) -> Rgb {
        Rgb {
            r: self.0[c.r as usize],
            g: self.0[c.g as usize],
            b: self.0[c.b as usize],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parse() {
        assert_eq!(Rgb::from_hex("#ff8000"), Some(Rgb::new(255, 128, 0)));
        assert_eq!(Rgb::from_hex("00ff00"), Some(Rgb::new(0, 255, 0)));
        assert_eq!(Rgb::from_hex("fff"), None);
        assert_eq!(Rgb::from_hex("+fffff"), None);
        assert_eq!(Rgb::from_hex("#12345g"), None);
    }

    #[test]
    fn scale_bounds() {
        let c = Rgb::new(255, 100, 1);
        assert_eq!(c.scale(255), c);
        assert_eq!(c.scale(0), Rgb::new(0, 0, 0));
    }

    #[test]
    fn gamma_endpoints() {
        let g = GammaLut::new(255, 2.2);
        assert_eq!(g.apply(Rgb::new(0, 255, 0)), Rgb::new(0, 255, 0));
        let half = GammaLut::new(128, 1.0);
        assert_eq!(half.apply(Rgb::new(255, 0, 0)).r, 128);
    }
}
