# DSP Notes

## M1 level processing

All level calculations use `f64` arithmetic. The level reference is 20 µPa;
normalised samples are represented in dBFS before the saved calibration offset
is applied.

The A and C weighting filters are cascades of bilinear-transformed second-order
sections using the IEC 61672-1 prototype frequencies. They are normalised at
1 kHz for every negotiated sample rate. Z weighting uses a 3 Hz second-order
Butterworth high-pass to remove DC. The planned oversampling stage is required
before claiming the specification's high-frequency accuracy target at 44.1 and
48 kHz; coefficient tuning must not substitute for it.

The normative class 1 limits are in IEC 61672-1:2013 Table 3 (frequency
weightings and acceptance limits; consulted as ČSN EN 61672-1 ed. 2). The ignored test
`class_1_limits_match_external_iec_fixture` checks them against a local CSV
made from that table; it has not been run yet (`docs/COMPLIANCE.md`). The
table itself is not reproduced in this repository. (Corrected 2026-11-05:
this note cited Table 2, which is the directional-response table.)

Fast and Slow time weighting use the IEC 61672-1 exponential mean-square
integrator with time constants 125 ms and 1 s. Leq accumulation uses Kahan
compensation, and peak is the maximum absolute weighted sample.

The calibration capture buffers eight seconds of raw samples on the engine
thread, after callback delivery. It uses a Hann-windowed narrow search within
±3% of the requested frequency, logarithmic parabolic peak refinement, and a
per-second in-phase/quadrature lock-in estimate at the detected frequency.
It rejects silence below −60 dBFS, clipping, invalid frequencies, captures
whose tone-to-background ratio is below 20 dB, and one-second tone-level
standard deviation of 0.1 dB or more. The offset proposal is
`L_cal − L_FS,tone`; it is independent of display weighting and is applied
only after explicit approval.

## Segment combination

Combined segment values follow specification 6.2: `Leq` is the energy mean
weighted by sample count and peak is the maximum segment sample peak.
`spl-engine`'s recorder tests verify this (specification 9.1 item 9). A
fixed-seed property test feeds random signals with up to twelve random
marker positions, including coincident and leading markers that leave empty
segments. The combined `Leq` matches a whole-session accumulator within
1e-9 dB, because segment values pass through `10·log10` and back. The combined
peak matches exactly. A square wave at −20 dBFS for 1 s followed by −40 dBFS
for 3 s gives those per-segment levels and the analytic combined value.

Each segment also keeps `LXFmax`, the largest Fast detector output after any
of its recorded samples; the combined value is the largest segment value.
Paused samples are excluded, and audio between a marker click and its label
entry counts towards the segment the marker opens. A recorder test places one
Fast excursion in each segment, including one during a pause and one after a
reserved marker click, and checks each segment's and the summary's maximum
exactly.

## RTA filter bank (M5)

Bands follow IEC 61260-1:2014's base-ten system: `G = 10^(3/10)`, exact
mid-band frequencies `1000 · G^(k/b)` and edges `fm · G^(±1/(2b))`, labelled
with the ISO 266 preferred frequencies. Bands whose upper edge reaches the
Nyquist frequency are left out (the 20 kHz one-third-octave band at 44.1 kHz).

Each band is a Butterworth band-pass with three poles per edge (6th order,
three sections), designed by the low-pass to band-pass transform with
pre-warped edges, so the digital −3 dB points fall exactly on the band edges
and the response equals the analog prototype at warped frequencies (checked
to 1e-6 dB). Band gain is normalised to 0 dB at the centre.

The bank decimates by two per octave with a 10th-order Butterworth
anti-aliasing low-pass at 0.17 of each stage's rate. A band runs at the
lowest rate where its upper edge is at most 1/8 (one-third octave) or 1/16
(octave) of the rate; its skirts then avoid both the bilinear warping near
Nyquist and the next anti-aliasing filter. A first design used 1/4: the
16 kHz octave band at 48 kHz then read an 8 kHz tone only 6.6 dB down, where
the analog response is 19.7 dB down. The highest bands do not fit at the
input rate, so the input is first oversampled by two as often as needed
(zero stuffing and a 10th-order interpolation low-pass at 0.27 of the
oversampled rate): four times for one-third octaves and eight times for
octaves at 48 kHz.

Measured deviation of each band's response from the analog 6th-order
Butterworth, over the passband and skirts down to −20 dB: at most 0.45 dB at
48 kHz and 0.33 dB at 44.1 kHz (one-third octaves), and 0.36 dB for octaves.
The deviations repeat with a band's position within its stage and come from
the remaining frequency warping.

Verification (specification 9.1 item 5):

- A sine at every band centre reads its level within ±0.2 dB in its band at
  44.1, 48 and 96 kHz (one-third octaves) and 48 kHz (octaves); both
  neighbouring bands are more than 10 dB lower (a sanity check, not the IEC
  limit).
- Deterministic white noise rises 1.0 dB per one-third octave from 200 Hz to
  10 kHz: every band within 0.4 dB of the line, slope within 0.03 dB.
- The IEC 61260-1:2014 class 1 relative attenuation limits (Table 1 for
  octaves, Table F.1 for one-third octaves) are checked by an
  ignored test against a local CSV made from those tables
  (`SPL_METER_IEC61260_TABLE_PATH`, columns
  `normalised_frequency,min_db,max_db`); the limits are not reproduced here
  and have not been run in this repository yet.

A release build processes one-third octaves from 200 Hz to 10 kHz about 33
times faster than real time on one core.


## Band log (ADR 0015)

The session recorder runs a filter bank of its own on the unweighted input
for the whole session. At every log row and marker it moves each band's
squared-sample sum and sample count (at the band's stage rate) into the
open interval and segment, then clears the bank's accumulators. A row's
level is `10·log10(Σx² / n) + C` over the interval; segments use their own
sums, so a segment value is exact rather than a recombination of rounded
rows. Combined values over a range weight each segment's band energy by its
broadband sample count, as for `LXeq`.

Verification (`spl-engine/tests/band_log.rs`): a 1 kHz sine at 0.1 FS rms
reads −20.00 dBFS in the 1 kHz octave band per row after the first second,
and 20 dB lower when the level drops by 20 dB; segment values match within
0.2 dB, except that the segment after a 20 dB drop reads about 0.2 dB high
from the band filter's decay. A marker labelled later splits the band
segments at its click time, and a recovered session continues its band log.

A release build runs the 31 one-third-octave bands from 20 Hz to 20 kHz at
48 kHz about 15 times faster than real time on one core (0.67 s for 10 s of
input), so recording them for a whole session costs about 7 % of a core.
