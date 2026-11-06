# ADR 0008: Pause and resume a recording

## Context

The specification's header had a "Start/Stop input" button, and its marker
kinds were start, stop, user, reset and interval_change. The project owner
wants to interrupt a recording without ending it, keep `start` and `stop` for
the very beginning and end, and keep timestamps correct across the
interruption.

## Decision

- Two new marker kinds, `pause` and `resume`. A header button (and
  <kbd>P</kbd>) toggles between them; it replaces "Start/Stop input".
- The input stream keeps running while paused. Fast and Slow stay live on the
  meter; Leq, sample peak and Fast maximum hold their values, and the recorder
  neither logs rows nor accumulates energy.
- `t_s` still derives from the sample count, and paused samples count. So
  `utc` after a pause is true wall-clock time, and the pause→resume segment
  has the real pause duration with zero samples and blank final values, which
  segment combination already skips.
- The log interval grid restarts at `resume`. The Log plot does not join
  traces across a pause.
- Reset, a calibrator Save (which resets) and an input switch start an
  unpaused recording.
- The file format goes to 1.1 (a backward-compatible addition), written as
  the string `"1.1"` so minor versions above 9 stay unambiguous.

## Consequences

Readers that hard-code the old marker kinds must accept the new ones; the
format already requires tolerating unknown values. Elapsed time and the
summary's duration include pauses; the measured time is the sum of the
non-empty segments.
