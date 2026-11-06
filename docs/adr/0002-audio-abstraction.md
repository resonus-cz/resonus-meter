# ADR 0002: Keep audio behind an abstraction

## Context

Measurement code must work with PipeWire on Linux and simulated sources in
tests without exposing platform APIs to the DSP layer.

## Decision

`spl-audio` owns the `AudioSource` boundary. Its production implementation
will use cpal 0.18 with PipeWire enabled, while deterministic sources support
tests and the demo.

## Consequences

The engine receives selected mono sample blocks only. Platform-specific audio
dependencies do not propagate into DSP, session, or UI code.
