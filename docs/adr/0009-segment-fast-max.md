# ADR 0009: Fast maximum as a segment final value

Amends specification decision 11.3 ("no Max … in the file").

## Context

Saved measurements held Leq and sample peak per segment, but the maximum Fast
level was shown on the meter only. For long measurements the owner needs the
maximum level of each marked part as well. A separate per-interval log of
Leq, peak and maximum was considered and rejected: log, markers and segments
already cover the workflow, and markers delimit the parts of interest.

## Decision

- Each segment stores `LAFmax`, `LCFmax` and `LZFmax`: the largest Fast level
  after any of its recorded samples. Empty segments (for example a pause)
  leave them blank. The summary adds `Fmax_db`, the largest segment value.
- The columns follow the existing `segments.csv` columns, so readers that
  use column positions keep working. This is a backward-compatible addition
  within file format 1.1 (unreleased, shared with ADR 0008).
- The Fast detector is not reset at markers or on resume, matching the meter.
  A segment's first few hundred milliseconds therefore carry decaying energy
  from before its start; this is documented, not compensated.
- Slow is still not stored.

## Consequences

A segment's maximum is available without a time series. Leq for a range that
was not delimited by markers during the measurement still cannot be derived
from the file; a short-Leq history can be added later if that is needed.
