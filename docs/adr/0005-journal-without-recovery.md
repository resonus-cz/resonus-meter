# ADR 0005: Session journal without crash recovery

Status: superseded by [ADR 0010](0010-journal-with-recovery.md).

Supersedes [ADR 0004](0004-no-session-journal.md), which misrecorded the
project owner's decision by dropping the journal together with recovery.

## Context

Specification section 6.6 called for a live on-disk journal, periodic
snapshots of the open segment's accumulators, and a "Recover previous
session" prompt on startup. The project owner wants the journal but does not
need recovery after an application crash.

## Decision

`spl-session` writes `log.csv`, `markers.csv` and `segments.csv` live into a
journal folder in the XDG state directory while a session runs. Saving copies
them into the bundle. RAM holds only what the plot needs, so long sessions do
not grow memory.

There is no recovery: no startup prompt, no `open_segment.json` snapshots and
no `truncated` segment flag.

## Consequences

- Memory stays bounded for multi-day sessions; the 24 h soak check keeps its
  constant-memory criterion.
- After a crash the unsaved session is not restored. Its journal CSVs are
  not offered to the user.
- The Log tab status row reports journal status (for example a write
  failure).
- The `kill -9` recovery check is not part of the verification plan.
