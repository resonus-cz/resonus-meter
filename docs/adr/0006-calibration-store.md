# ADR 0006: Calibration store in spl-engine

## Context

Specification 4.9 (steps 6–8) requires saved calibrations to persist per
input profile, (device id, channel), as an append-only history from which
older records can be restored. ADR-4 in the specification chooses TOML via
`serde` for settings in the XDG config directory.

## Decision

- `spl-engine` owns the store (`calibration_store` module), as the spec's
  layout assigns "calibration store" to the engine. It takes the file path as
  an argument; the app resolves it with `glib::user_config_dir()`, so Flatpak
  builds use their sandboxed config directory without special code.
- One file, `calibrations.toml`, with a top-level `version` (settings schema
  version 1) and an array of `[[calibration]]` records. Unknown keys are
  ignored and a higher version is rejected; migrations live in that module.
- Every save appends a record and rewrites the file atomically (temporary
  file, `fsync`, rename). Restoring appends a copy with a new `saved_utc` and
  the original `calibrated_utc`. The newest record of a profile is active.
- An unreadable file is reported in the Calibration page and is never
  overwritten; calibration then works for that run only.
- The demo source is a profile like any other input (`demo.sine-1khz`,
  channel 1), which keeps the code free of special cases.
- Manual offset edits apply at once and are saved after 1.5 s without further
  change, so stepping with the ±0.1 dB buttons does not fill the history.
- Dependencies: `serde` and `toml` (ADR-4), and `thiserror` for the error
  type (contributor guidelines).

## Consequences

The engine, not the app, defines the record format, so a future Android UI
shares it. The file grows by one record per saved calibration, which is
negligible. A device whose backend id changes between sessions starts
uncalibrated; that is preferable to applying another microphone's offset.
