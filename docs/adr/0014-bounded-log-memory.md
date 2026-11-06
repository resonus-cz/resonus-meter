# ADR 0014: The journal holds the log; memory holds a plotting window

Amends [ADR 0010](0010-journal-with-recovery.md).

## Context

ADR 0010 kept every log row in memory as well as in the journal: about
45 MB for a 24 h session at 0.125 s. The Log view also copied the whole log
every display refresh (125 ms), so a long session cost both memory and CPU.
The RTA log planned next would add one value per band to every row.

## Decision

- Once a journal flush has appended a row to `log.csv`, the recorder forgets
  it. Memory holds only rows not yet journaled. A session without a journal
  (its folder could not be created) keeps all its rows in memory, so it can
  still be saved.
- Saving streams the journaled rows from `log.csv` into the bundle, then adds
  the rows still in memory. Range saves filter the streamed rows in the same
  way as the in-memory rows.
- Recovery truncates `log.csv` to the row count in `recovery.toml` instead of
  rewriting it, and streams the rows into the Log view's history.
- The Log view's history keeps the last hour (the longest finite window) at
  full resolution, and an overview of the whole session of at most 4 096
  points. When the overview is full, neighbouring points are merged. A point
  keeps the highest A, C and Z levels of its rows, so short loud events stay
  visible. **All** shows full resolution until the session is longer than an
  hour, then the overview. The engine returns only the rows of the visible
  window.
- A marker whose label dialog is open is noted in the first row after its
  time, like other markers. Rows after its time stay out of the journal until
  the marker is placed, so that row can still change.

## Consequences

- Memory for a 24 h session is a few megabytes instead of about 45 MB, and
  each refresh copies at most an hour of rows.
- Saving a long session reads the journal back from disk.
- **All** on a session longer than an hour shows maxima per point, not every
  row. The saved files are unaffected.
- A row at exactly a reserved marker's time is no longer the one that notes
  the marker; the next row is, as for markers placed directly.
