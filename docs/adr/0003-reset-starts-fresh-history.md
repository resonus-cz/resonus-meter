# ADR 0003: Reset starts a fresh recording history

## Context

The initial session design retained the log and saved history across a meter
reset. This made the plot and exported CSV contain levels from before and after
the reset, despite the meter values and elapsed-time display restarting.

## Decision

Reset clears the current recording history as well as the meter accumulators
and elapsed time. The replacement history begins at `t_s = 0` with a `Reset`
marker. User markers reserve their timestamp when the marker action is invoked,
not when a later label dialog is confirmed.

## Consequences

Saving after Reset exports only post-reset data. A user who needs the earlier
history must save it before resetting.
