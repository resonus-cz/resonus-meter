# Compliance matrix

Traceability from each normative requirement to code and tests. A requirement without a
passing test is not implemented. Sources: the owner's copies of the standards (never
committed); editions as listed. Clause numbers
were checked against the text on 2026-11-05 (ČSN EN versions of the IEC standards).

**Not a certified instrument.** Implementing the methods of IEC 61672-1 is not the same as a
class 1 sound level meter: that needs pattern evaluation of the whole measuring chain
(microphone, interface, software; IEC 61672-2). Say so wherever results are reported.

| Standard, edition | Clause / table | Requirement | Code | Test | Status |
|---|---|---|---|---|---|
| IEC 61672-1:2013 | 5.5, Annex E (C: Eq. (E.1), A: Eq. (E.6), Z: Eq. (E.9); pole frequencies E.4.1) | A, C and Z frequency weightings | `spl-dsp/src/weighting.rs` | `weighting.rs::the_analytic_response_matches_the_specification_sanity_values`, `a_weighting_matches_reference_sanity_values`, `c_weighting_matches_reference_sanity_values` | partly: spot values only (A at 100 Hz, 1 kHz, 10 kHz; C at 100 Hz, 10 kHz) |
| IEC 61672-1:2013 | 5.5.2, Table 3 | weighting values and class 1 acceptance limits, 10 Hz-20 kHz | `spl-dsp/src/weighting.rs` | `weighting.rs::class_1_limits_match_external_iec_fixture` (ignored: needs a local table, `SPL_METER_IEC_TABLE_PATH`) | **open**: never run; the fixture can be made from the library copy. High-frequency accuracy at 44.1/48 kHz needs the planned oversampling (DSP_NOTES) |
| IEC 61672-1:2013 | 5.8.1 | exponential time constants 0.125 s (F) and 1 s (S) | `spl-dsp/src/detector.rs` (`TimeWeighting::time_constant_s`) | `detector.rs::fast_step_reaches_sixty_three_percent_at_time_constant`, `slow_step_reaches_sixty_three_percent_at_its_time_constant` | verified (step response) |
| IEC 61672-1:2013 | 5.9, Table 4 | tone-burst response | `spl-dsp/src/detector.rs` | none | **open** |
| IEC 61672-1:2013 | 3 (definitions) | time-averaged level Leq (energy mean), peak level (maximum absolute weighted sample) | `spl-dsp/src/detector.rs` (`LevelAccumulator`) | `detector.rs::leq_and_peak_match_a_sine`, `leq_is_an_energy_mean_for_two_levels` | verified. Peak is the sample peak, not oversampled (SPEC 4.5) |
| IEC 61672-1:2013 | 5.11 | overload indication | `spl-engine/src/engine/mod.rs` (clip flag, `spl_dsp::units::CLIP_LEVEL` = 1 - 2^-15, the stack's value) | `spl-engine/tests/engine.rs::engine_reports_calibrated_sine_level_and_clip` | implemented as digital clipping of the input; analogue overload of the interface is not detectable |
| IEC 61672-1:2013 | 5.2 | adjustment at the calibration check frequency | `spl-dsp/src/calibration.rs` (`propose`), `spl-engine` calibration store (ADR 0006) | `calibration.rs::*`, `spl-engine/tests/engine.rs::automatic_capture_collects_raw_samples_before_applying_a_proposal` | verified (method; resonus-core `calibration` ports it identically, D-C1) |
| IEC 61672-3:2013 | 9.1, 10.2 | manufacturer's adjustment data; ambient correction of the calibrator level | not implemented in the meter | - | **open**: resonus-core `calibration` has both corrections (D-C1); the meter's record format accepts the fields (schema version 1) but the app neither asks for nor applies them |
| IEC 60942:2017 | 5.1.5, 5.1.7, Table 1 | only class LS/M and 1/M pistonphones need a (static-pressure) correction | - | - | informative for the user (calibration dialog text), see the row above |
| IEC 61260-1:2014 | 5.2 Eq. (1), 5.4 Eqs. (2)-(3), 5.6 Eqs. (4)-(5) | base-ten octave ratio G = 10^(3/10), exact mid-band and edge frequencies | `spl-dsp/src/bands.rs` | `bands.rs::the_octave_ratio_is_ten_to_the_three_tenths`, `third_octaves_from_20_hz_to_20_khz_follow_the_base_ten_series` | verified |
| IEC 61260-1:2014 | 5.5, Annex E (Table E.1) | nominal mid-band frequencies (labels) | `spl-dsp/src/bands.rs` | `bands.rs::octaves_use_every_third_one_third_octave_label`, `the_last_index_names_the_table_end` | verified for 20 Hz-20 kHz |
| IEC 61260-1:2014 | 5.10, Table 1 (octaves), Annex F Table F.1 (one-third octaves) | class 1 relative attenuation limits | `spl-dsp/src/filterbank.rs` | `filterbank/tests.rs` (centre-frequency level, noise slope; sanity checks), class 1 limits: ignored test with `SPL_METER_IEC61260_TABLE_PATH` | **open**: never run |
| ISO 1683:2015 | Table 1 | reference sound pressure 20 µPa | `spl-dsp/src/units.rs` (`REFERENCE_PRESSURE_PA`) | `units.rs::full_scale_sine_rms_is_minus_3_dbfs` (dB FS convention) | verified |

Open items in this table are scheduled in the project's task list.
