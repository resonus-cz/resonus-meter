# ADR 0007: Reopen the most recently used input

## Context

The app always started with the demo source, so every session began with a
trip to the Sound Device page. Specification ADR-2 (rule 4) forbids silently attaching to
the system default source; it does not cover the user's own previous choice.
A new setting needs an ADR note (specification section 7, rule 5).

## Decision

- `spl-engine` gains a `settings` module and `settings.toml` in the XDG config
  directory (settings schema version 1, ADR-4). It is the home for later
  settings such as the journal write interval.
- The file keeps up to five recently used inputs, most recent first. An input
  is (device id, channel), as for calibration profiles, plus the requested
  sample rate. Every input selected on the Sound Device page, including the
  demo source, moves to the front.
- At start the app opens the first remembered input that is present with that
  channel and rate; an unplugged or failing one falls back to the next, and
  finally to the demo source. A toast says so when the last used input was not
  opened. This is priority by recency only; usage counts would add state
  without a clear benefit.
- `--demo` always starts with the demo source, so packaging smoke tests do not
  depend on the host's devices.
- An unreadable `settings.toml` is reported and never overwritten.

## Consequences

The resolved input is always shown in the header subtitle, so the choice is
explicit, not silent. Devices whose backend id changes between sessions are
not found again; that matches how calibration profiles behave (ADR 0006).
