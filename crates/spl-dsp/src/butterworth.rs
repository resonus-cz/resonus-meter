//! Butterworth low-pass and band-pass designs as second-order sections.
//!
//! Analog prototypes are mapped with the bilinear transform after
//! pre-warping the band edges, `ω = 2·fs·tan(π·f/fs)`, so the digital edges
//! fall exactly on the requested frequencies.

use std::f64::consts::PI;

use crate::sos::{Biquad, cascade_magnitude};

/// A low-pass of even `order` with its −3 dB point at `cutoff_hz`.
pub(crate) fn low_pass(order: usize, cutoff_hz: f64, sample_rate_hz: f64) -> Vec<Biquad> {
    let cutoff = prewarp(cutoff_hz, sample_rate_hz);
    prototype_poles(order)
        .filter(|pole| pole.im > 0.0)
        .map(|pole| {
            // (s − ωc·p)(s − ωc·p̄) = s² − 2ωc·Re(p)·s + ωc².
            Biquad::from_analog(
                [0.0, 0.0, cutoff * cutoff],
                [1.0, -2.0 * cutoff * pole.re, cutoff * cutoff],
                sample_rate_hz,
            )
        })
        .collect()
}

/// A band-pass with `order_per_edge` poles per edge (3 for IEC 61260-1
/// bands, as in pyfilterbank), −3 dB at `lower_hz` and `upper_hz`, and unit
/// gain at their geometric mean.
pub(crate) fn band_pass(
    order_per_edge: usize,
    lower_hz: f64,
    upper_hz: f64,
    sample_rate_hz: f64,
) -> Vec<Biquad> {
    let lower = prewarp(lower_hz, sample_rate_hz);
    let upper = prewarp(upper_hz, sample_rate_hz);
    let width = upper - lower;
    let centre_squared = lower * upper;
    let mut sections = Vec::new();
    for pole in prototype_poles(order_per_edge).filter(|pole| pole.im >= 0.0) {
        // Low-pass to band-pass, s → (s² + ω0²)/(B·s): each prototype pole p
        // becomes the roots of s² − p·B·s + ω0² = 0.
        let half = pole.scale(width / 2.0);
        let root = (half * half - Complex::real(centre_squared)).sqrt();
        let roots = [half + root, half - root];
        if pole.im == 0.0 {
            // A real prototype pole gives one conjugate pair of band-pass poles.
            sections.push(section(roots[0], width, sample_rate_hz));
        } else {
            for root in roots {
                sections.push(section(root, width, sample_rate_hz));
            }
        }
    }
    let centre_hz = (sample_rate_hz / PI) * (centre_squared.sqrt() / (2.0 * sample_rate_hz)).atan();
    let gain = cascade_magnitude(&sections, centre_hz, sample_rate_hz);
    let first = &mut sections[0];
    first.b0 /= gain;
    first.b1 /= gain;
    first.b2 /= gain;
    sections
}

/// B·s / ((s − r)(s − r̄)): one band-pass pole pair with a zero at DC and one
/// at infinity, which map to z = 1 and z = −1.
fn section(root: Complex, width: f64, sample_rate_hz: f64) -> Biquad {
    Biquad::from_analog(
        [0.0, width, 0.0],
        [1.0, -2.0 * root.re, root.re * root.re + root.im * root.im],
        sample_rate_hz,
    )
}

/// Poles of the normalised analog Butterworth low-pass of `order`.
fn prototype_poles(order: usize) -> impl Iterator<Item = Complex> {
    #[allow(clippy::cast_precision_loss)]
    let order_f = order as f64;
    (0..order).map(move |index| {
        #[allow(clippy::cast_precision_loss)]
        let angle = PI * (2.0 * index as f64 + order_f + 1.0) / (2.0 * order_f);
        Complex {
            re: angle.cos(),
            // Exact zero for the real pole of odd orders.
            im: if (angle - PI).abs() < 1e-12 {
                0.0
            } else {
                angle.sin()
            },
        }
    })
}

fn prewarp(frequency_hz: f64, sample_rate_hz: f64) -> f64 {
    2.0 * sample_rate_hz * (PI * frequency_hz / sample_rate_hz).tan()
}

#[derive(Debug, Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    fn scale(self, factor: f64) -> Self {
        Self {
            re: self.re * factor,
            im: self.im * factor,
        }
    }

    /// Principal square root.
    fn sqrt(self) -> Self {
        let modulus = self.re.hypot(self.im);
        let re = f64::midpoint(modulus, self.re).sqrt();
        let im = ((modulus - self.re) / 2.0).sqrt().copysign(self.im);
        Self { re, im }
    }
}

impl std::ops::Add for Complex {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }
}

impl std::ops::Sub for Complex {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

impl std::ops::Mul for Complex {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{band_pass, low_pass};
    use crate::sos::cascade_magnitude;

    fn db(magnitude: f64) -> f64 {
        20.0 * magnitude.log10()
    }

    #[test]
    fn the_low_pass_is_flat_and_minus_three_db_at_cutoff() {
        let sections = low_pass(10, 8_000.0, 48_000.0);

        assert_eq!(sections.len(), 5);
        assert!(db(cascade_magnitude(&sections, 10.0, 48_000.0)).abs() < 1e-9);
        assert!((db(cascade_magnitude(&sections, 8_000.0, 48_000.0)) + 3.0103).abs() < 1e-3);
    }

    #[test]
    fn the_band_pass_matches_the_analog_butterworth_response() {
        // A 1 kHz one-third-octave band far below Nyquist, where the bilinear
        // transform's warping is negligible.
        let (lower, upper) = (891.250_938, 1_122.018_454);
        let sections = band_pass(3, lower, upper, 48_000.0);
        let centre = (lower * upper).sqrt();

        assert_eq!(sections.len(), 3);
        let warp = |frequency: f64| super::prewarp(frequency, 48_000.0);
        let (lower_w, upper_w) = (warp(lower), warp(upper));
        let centre_w = (lower_w * upper_w).sqrt();
        for frequency in [
            500.0, 800.0, lower, 950.0, centre, 1_050.0, upper, 1_260.0, 2_000.0,
        ] {
            // The digital filter equals the analog prototype at pre-warped
            // frequencies: |H|² = 1 / (1 + Ω⁶), Ω = (ω/ω0 − ω0/ω) / (B/ω0).
            let w = warp(frequency);
            let omega = (w / centre_w - centre_w / w) / ((upper_w - lower_w) / centre_w);
            let expected = -10.0 * (1.0 + omega.powi(6)).log10();
            let actual = db(cascade_magnitude(&sections, frequency, 48_000.0));
            assert!(
                (actual - expected).abs() < 1e-6,
                "{frequency} Hz: {actual} vs {expected}"
            );
        }
        // The band edges are −3 dB.
        assert!((db(cascade_magnitude(&sections, lower, 48_000.0)) + 3.0103).abs() < 1e-3);
        assert!((db(cascade_magnitude(&sections, upper, 48_000.0)) + 3.0103).abs() < 1e-3);
    }
}
