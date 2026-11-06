# ADR 0015: Band levels in saved measurements

Amends [ADR 0011](0011-rta.md).

## Context

The project owner asked for the RTA bands in saved measurements: a time log
and per-segment values with their combination over a range, like the
broadband files. They chose the quantity: the band log is always
**Z-weighted**, since A or C weighting can be applied per band afterwards,
and it holds the equivalent level over each log interval, following the log
interval and its changes. Sampling Fast or Slow at the log interval would
miss what happens between rows; interval Leq covers every moment once and
combines exactly into segment values.

## Decision

- File format 1.2 adds `rta_log.csv` (band `LZeq` per `log.csv` row, same
  `t_s`), `rta_segments.csv` (band `LZeq` per segment), an `rta` front-matter
  block (`quantity: LZeq`, `bands_per_octave`, `bands_hz`) and
  `rta_LZeq_db` in the summary. Bundles without band levels omit them;
  readers of 1.1 ignore them.
- A row holds the band `LZeq` over the measured time since the previous row:
  normally one interval, longer after a log-interval change or a pause.
- The recorder owns a second filter bank (the bands of the RTA
  preferences) that runs for the whole session, independent of the RTA tab.
  The bands are fixed for one recording history; a preference change applies
  from the next Reset or New Session, or at once while nothing has been
  recorded.
- A new RTA preference, **Record band levels**, on by default (`record` in
  `[rta]`, schema version stays 1; a missing key reads as on).
- Band rows are journaled to `rta_log.csv` and leave memory like log rows
  (ADR 0014); `recovery.toml` keeps the open interval and segment energy
  and gains an optional `bands` table, so older journals still recover.

## Consequences

- About 7 % of one core for 31 one-third-octave bands; twice that while the
  RTA tab also shows its own bank.
- A 24 h session at 0.125 s writes about 690 000 band rows, roughly 170 MB
  of CSV for 31 bands; the log interval controls the size.
- The RTA tab's display Leq and the saved band Leq are separate: the tab's
  starts when it is shown, the saved one covers the recording.
