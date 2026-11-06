//! Tick positions on the Log plot's time axis (planned item "Log time ticks").
//!
//! Ticks sit on whole multiples of a round step, so a 13 s window shows 0, 2, 4, ...
//! instead of fifths of the window. Design choice, no standard applies.

/// Round time steps in seconds, smallest first: 1-2-5 seconds, then the clock's
/// divisions (10, 15, 30 s; 1, 2, 5, 10, 15, 30 min; 1, 2, 3, 6, 12 h; 1 day).
pub(crate) const TIME_STEPS_S: [f64; 19] = [
    1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1_800.0, 3_600.0, 7_200.0,
    10_800.0, 21_600.0, 43_200.0, 86_400.0, 172_800.0,
];

/// Returns the tick times in seconds for the window `start_s..=end_s`.
///
/// The step is the smallest entry of [`TIME_STEPS_S`] for which the window holds at
/// most `max_ticks` ticks; if none does, the largest entry times the smallest whole
/// factor that does. Ticks are the multiples `k * step` (integer `k`) with
/// `start_s - 1e-9 <= k * step <= end_s + 1e-9`, increasing.
///
/// Returns an empty vector if `max_ticks` is 0, if `end_s < start_s`, or if either
/// bound is not finite.
pub(crate) fn time_ticks(start_s: f64, end_s: f64, max_ticks: usize) -> Vec<f64> {
    if max_ticks == 0 || !start_s.is_finite() || !end_s.is_finite() || end_s < start_s {
        return Vec::new();
    }
    // Indices of the first and last multiple of `step` inside the window; the window
    // is far below 2^53 s, so the casts are exact.
    #[allow(clippy::cast_possible_truncation)]
    let range = |step: f64| {
        let first = ((start_s - 1e-9) / step).ceil() as i64;
        let last = ((end_s + 1e-9) / step).floor() as i64;
        (first, last)
    };
    // `max_ticks` is a pixel count, far below i64::MAX.
    #[allow(clippy::cast_possible_wrap)]
    let fits = |step: f64| {
        let (first, last) = range(step);
        last - first < max_ticks as i64
    };
    let largest = TIME_STEPS_S[TIME_STEPS_S.len() - 1];
    let step = TIME_STEPS_S
        .iter()
        .copied()
        .find(|&step| fits(step))
        .or_else(|| {
            (2..=u32::MAX)
                .map(|f| largest * f64::from(f))
                .find(|&step| fits(step))
        })
        .unwrap_or(largest);
    let (first, last) = range(step);
    // Tick indices are small integers (see above).
    #[allow(clippy::cast_precision_loss)]
    (first..=last).map(|k| k as f64 * step).collect()
}

#[cfg(test)]
#[path = "ticks_tests.rs"]
mod tests;
