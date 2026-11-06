# ADR 0010: Session journal with crash recovery

Supersedes [ADR 0005](0005-journal-without-recovery.md).

## Context

Some measurements run for up to 24 hours. ADR 0005 kept a journal but no
recovery; the project owner now wants a crashed or killed run to be
recoverable, and the measurement to continue paused rather than resume on
its own.

## Decision

- Each running session journals into its own folder,
  `$XDG_STATE_HOME/cz.resonus.SplMeter/sessions/session-<start ms>/` (the id
  `io.github.vyhyb.SPLMeter` before ADR 0016)
  (`~/.local/state/…`, or the Flatpak sandbox's state folder). The state
  directory survives reboots and power loss, unlike `/tmp`.
- The folder has the layout of a saved bundle. `log.csv` is appended;
  `markers.csv`, `segments.csv` and `measurement.md` are replaced atomically
  (temporary file, `fsync`, rename) and show the session as if saved at that
  moment. `recovery.toml`, written last, holds the exact recorder and meter
  state, including the open segment's accumulators, and the number of
  `log.csv` rows that belong to it.
- The journal is written at a configurable interval: 10 s, 1 min (default) or
  5 min, in a new General preferences page, and immediately whenever the
  markers change (Add Marker, Pause, Resume, log-interval change, Reset).
  A crash loses at most one interval of log rows, never a marker.
- On start, the app lists journals before starting its own and offers the
  newest: **Recover**, **Not Now** (kept for the next start) or **Discard**.
  Recover reopens the recorded input at its sample rate, restores the
  recorded calibration, and continues the measurement **paused**: a `pause`
  marker labelled "Pause (recording interrupted)" is placed where the journal
  ends, and the session clock moves on to the current time, so the gap is a
  pause with true wall-clock timestamps. The Log view then shows the whole
  session. Unreadable journals are reported and kept.
- Closing the window with changes since the last save asks
  **Cancel / Discard / Save…**. A clean close, Reset and an input switch delete
  the journal of the recording they end.
- `recovery.toml` is internal (versioned, `RECOVERY_VERSION = 1`); it is not
  part of the file format and is never copied into saved bundles.

## Consequences

- The `kill -9` recovery check returns to the verification plan.
- Journal writes run on the UI thread every interval; they are small, but an
  `fsync` on a slow disk can delay one display refresh.
- RAM still holds the whole log, about 64 bytes per row (a 24 h session at
  0.125 s is roughly 45 MB). Bounding it to a plotting window is a planned item.
- Only the newest journal is offered per start; older ones wait for later
  starts.
