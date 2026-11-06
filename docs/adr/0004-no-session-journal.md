# ADR 0004: No crash-safe session journal in v1

Status: superseded by [ADR 0005](0005-journal-without-recovery.md).

## Context

Specification section 6.6 called for a live on-disk journal in the XDG state
directory, periodic snapshots of the open segment, and a "Recover previous
session" prompt on startup. Milestone M4 listed "recovery works" as a
completion criterion. The project owner decided that crash recovery is not
needed.

## Decision

v1 has no session journal and no recovery. The recording history is held in
memory and written to disk only when the user saves a measurement bundle.
`spl-session` keeps the atomic `measurement.md` write (temp file, `fsync`,
rename), so a completed bundle is still never partially valid.

## Consequences

- An application crash, power loss or `kill` loses the unsaved recording.
  Users who measure for long periods should save at intervals.
- Memory grows with session length instead of being bounded by the plotting
  window. One in-memory log row is about 72 bytes, so a day of recording takes
  roughly 6 MB at the default 1 s interval and 50 MB at 0.125 s. This is
  acceptable for the intended use; revisit if multi-week sessions are needed.
- The Log tab status row shows no journal status, and the `kill -9` recovery
  check is removed from the verification plan.
- No `truncated` segment flag is needed in file format v1.
