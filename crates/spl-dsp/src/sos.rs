//! Second-order sections (biquads) in direct form II transposed.

use std::f64::consts::PI;

/// State magnitudes below this are flushed to zero, so silent low bands do
/// not slow down on subnormal numbers (specification ADR-3).
const FLUSH_BELOW: f64 = 1e-30;

#[derive(Debug, Clone)]
pub(crate) struct Biquad {
    pub(crate) b0: f64,
    pub(crate) b1: f64,
    pub(crate) b2: f64,
    a1: f64,
    a2: f64,
    state_1: f64,
    state_2: f64,
}

impl Biquad {
    /// Maps an analog section `[s², s, 1]` with the bilinear transform.
    pub(crate) fn from_analog(
        numerator: [f64; 3],
        denominator: [f64; 3],
        sample_rate_hz: f64,
    ) -> Self {
        let scale = 2.0 * sample_rate_hz;
        let numerator = bilinear(numerator, scale);
        let denominator = bilinear(denominator, scale);
        Self {
            b0: numerator[0] / denominator[0],
            b1: numerator[1] / denominator[0],
            b2: numerator[2] / denominator[0],
            a1: denominator[1] / denominator[0],
            a2: denominator[2] / denominator[0],
            state_1: 0.0,
            state_2: 0.0,
        }
    }

    pub(crate) fn process(&mut self, input: f64) -> f64 {
        let output = self.b0.mul_add(input, self.state_1);
        self.state_1 = self.b1.mul_add(input, self.state_2 - self.a1 * output);
        self.state_2 = self.b2 * input - self.a2 * output;
        if self.state_1.abs() < FLUSH_BELOW {
            self.state_1 = 0.0;
        }
        if self.state_2.abs() < FLUSH_BELOW {
            self.state_2 = 0.0;
        }
        output
    }

    pub(crate) fn reset(&mut self) {
        self.state_1 = 0.0;
        self.state_2 = 0.0;
    }

    /// Magnitude at the normalised angular frequency `angular` (radians per sample, negated).
    pub(crate) fn magnitude(&self, angular: f64) -> f64 {
        let z1 = (angular.cos(), angular.sin());
        let z2 = ((2.0 * angular).cos(), (2.0 * angular).sin());
        let numerator = complex_magnitude(
            self.b0 + self.b1 * z1.0 + self.b2 * z2.0,
            self.b1 * z1.1 + self.b2 * z2.1,
        );
        let denominator = complex_magnitude(
            1.0 + self.a1 * z1.0 + self.a2 * z2.0,
            self.a1 * z1.1 + self.a2 * z2.1,
        );
        numerator / denominator
    }
}

/// A cascade of sections with its magnitude response.
pub(crate) fn cascade_magnitude(
    sections: &[Biquad],
    frequency_hz: f64,
    sample_rate_hz: f64,
) -> f64 {
    let angular = -2.0 * PI * frequency_hz / sample_rate_hz;
    sections
        .iter()
        .map(|section| section.magnitude(angular))
        .product()
}

fn bilinear(coefficients: [f64; 3], scale: f64) -> [f64; 3] {
    let [quadratic, linear, constant] = coefficients;
    [
        quadratic * scale * scale + linear * scale + constant,
        -2.0 * quadratic * scale * scale + 2.0 * constant,
        quadratic * scale * scale - linear * scale + constant,
    ]
}

fn complex_magnitude(real: f64, imaginary: f64) -> f64 {
    real.hypot(imaginary)
}
