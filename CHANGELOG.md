# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-11-06

### Changed

- The calibration note in the ribbon spans both ribbon rows with a fixed width
  (10 characters, up to three wrapped lines, the dB value on its own line), so its
  text no longer shifts or widens the ribbon.
- Style tokens pinned to the resonus-core tag v0.10.0 (was commit a4cc7af; the
  file is identical).
- Application id `cz.resonus.SplMeter` (was `io.github.vyhyb.SPLMeter`);
  settings, calibrations and session journals of the former id are moved once
  at startup (ADR 0016).
- Saved bundles name the software "Re:sonus Meter <version>" (was "SPL Meter
  <version>"; `spl_engine::SOFTWARE_NAME`). Readers that match the old name must
  accept both.
- Licence AGPL-3.0-only (was GPL-3.0-or-later), as the other Re:sonus apps.
- Clipping (overload flag and calibration) at `|x| >= 1 - 2^-15`, the top
  16-bit code, the Re:sonus stack's value (was 0.999).
- Window layout of the Re:sonus stack (ADR 0017): brand and centred "Meter"
  title in the header bar, the controls in a ribbon (Measure, Reading,
  Device, Files), Log and RTA as view-switcher tabs in a framed panel, a
  bottom tab bar below 400 sp. Window title "Re:sonus Meter", default size
  940 × 860.
- Plots in the stack's plot style: colours from the resonus-core style
  tokens (light and dark, following the system), text in Lexend at the token
  sizes, marker lines in the neutral colour, the "Re:sonus" plot mark bottom
  right.

### Added

- `docs/COMPLIANCE.md`: standards requirements mapped to code and tests.
- Ribbon shortcuts to the Sound Device and Calibration preferences pages, and
  the calibration status ("Uncalibrated", "Manual offset …", "94.0 dB at
  1000 Hz") in the ribbon.
- `--screenshot=PATH` and `--screenshot-delay=SECONDS` render the window to a
  PNG and quit (README screenshots).

- Band levels in saved measurements (file format 1.2): `rta_log.csv` holds
  the Z-weighted band Leq over each log interval and `rta_segments.csv` the
  band Leq per segment; the summary combines them over the saved range. A
  new **Record band levels** RTA preference (on by default) turns it off.
  The RTA tab shows the log interval selector too, linked to the Log tab's.

- Level axes of the Log and RTA plots follow the data with hysteresis instead
  of jumping every frame. Scroll (or Up/Down) moves the axis, Ctrl+scroll (or
  +/−) zooms, and **Auto Scale** returns to the automatic range.
- An **RTA** tab with octave or one-third-octave bars (IEC 61260-1 base-ten
  bands, 6th-order Butterworth filters), Fast, Slow or Leq detector, A, C or Z
  display weighting and peak hold, plus an RTA preferences page.
- Marker labels and notes can be edited: click a marker line in the Log
  plot, or use **Markers…** below it.
- A primary menu with **New Session** (<kbd>Ctrl</kbd>+<kbd>N</kbd>, asks first
  when there are unsaved changes) and Preferences; it replaces the gear
  button. After saving, a toast offers **Open Folder**.
- Attachments in the save dialog: files are copied into the measurement's
  `attachments/` folder and listed with a SHA-256 in `measurement.md`.
- The save dialog can save the part of a session between two markers and
  previews its LAeq, LCeq and LZeq before saving.
- An independent Python reader, `docs/examples/read_session.py`, with a
  round-trip test against saved bundles.
- Crash recovery: unsaved measurements are journaled to the XDG state
  directory at an interval set in the new General preferences page (10 s,
  1 min or 5 min). The next start offers to recover a journal; the
  measurement continues paused. Closing with unsaved changes asks first.
- Fast maximum (`LAFmax`, `LCFmax`, `LZFmax`) per segment in
  `segments.csv` and in the saved summary.
- **Pause/Resume** (header button or <kbd>P</kbd>) interrupts a recording
  without ending it. Paused time is excluded from Leq, peak and maximum but
  keeps timestamps true to wall-clock time; bundles record `pause` and
  `resume` markers (file format 1.1).
- The app reopens the most recently used input on start, falling back to
  earlier inputs and then to the demo source (with a notice) when it is not
  available. Recent inputs are kept in `settings.toml`; `--demo` always starts
  with the demo source.
- Persistent calibration per input (device and channel) in an append-only
  `calibrations.toml` history. Switching input applies that input's latest
  calibration; the Calibration page shows the history with **Restore**, and
  calibrator captures can carry a note.
- Automatic calibrator capture in Calibration preferences. It collects eight
  seconds of raw audio outside the callback, refines a tone within ±3% of the
  requested frequency, reports quality checks and an unapplied proposal, and
  requires an explicit **Save** before the active measurement receives
  calibrator calibration metadata.
- A Save Measurement dialog that collects a bundle name, Markdown description,
  and destination folder. Exports use the chosen metadata without interrupting
  the live recording.
- A native Log view below the meter with a live Fast A/C/Z time plot,
  toggleable traces, 1 minute/10 minute/1 hour/full-session windows, labeled
  marker lines, recording status, selectable interval, and an **Add Marker**
  header action. Reset and interval changes remain segment markers in saved
  session bundles.
- UI-independent engine session recording with interval Fast A/C/Z logs,
  reset-delimited segments, and portable bundle saving.
- Portable `splmeter-session` v1 bundles with Fast level logs, markers, final
  A/C/Z segment values, and an atomic Markdown measurement summary.
- Main meter surface with A/C/Z and Fast/Slow/Leq/Peak selectors, held Fast
  maximum, resettable elapsed time, and text/icon calibration and overload
  indicators.
- Preferences dialog with Sound Device and Calibration pages, including a
  manual in-memory demo calibration offset and full-scale sine equivalence.
- Live cpal device binding in the M3 application. Sound Device preferences
  enumerate inputs alongside the demo source and immediately open the chosen
  device, channel, and supported rate; configured-rate, stream-error, and
  dropped-sample status is visible to the user.

### Changed

- Long sessions no longer keep their whole log in memory. Journaled rows are
  read back from the journal when saving. For sessions over an hour, the Log
  view's **All** window shows an overview in which each point holds the
  highest levels of the rows it combines.
- Manual calibration offsets are saved to the current input's history once
  they stop changing, instead of being kept in memory only.
- Calibration capture now shows raw sample-peak headroom separately from the
  calibrated full-scale SPL reading, and documents that PipeWire/GNOME input
  volume remains upstream of capture.
- Approving a captured calibration starts a fresh calibrated recording history
  rather than recalibrating prior rows.
- Reset preserves the selected Log-tab interval and synchronizes the selector
  with the fresh recording history.
- Reset now clears the active recording history as well as the meter display;
  subsequent saves contain only post-reset samples, beginning at a `reset`
  marker.
- Marker timestamps are reserved when **Add Marker** is invoked, rather than
  when its label dialog is submitted.

### Fixed

- The save dialog's **Save** and **Choose…** buttons did nothing: the
  dialog's state was freed as soon as it opened.
- Saving failed during the first journal interval of a session that records
  band levels; the save dialog now shows save errors in a banner at its top
  instead of below the scrolled content.
- The recovery prompt no longer returns on every start: empty journals are
  deleted silently, several leftovers are offered in one dialog, and
  **Discard All** removes them all.
- The demo source generates audio as wall-clock time passes; it ran at
  0.8× real time, so demo timestamps drifted behind the clock.
- Bundles saved after switching input no longer report a calibrator
  calibration as manual, and keep the calibration's original timestamp.
- Closing Preferences stops the calibration page's capture progress timer.

## [0.1.0] - 2026-10-18

### Added

- Fast maximum (`LAFmax`, `LCFmax`, `LZFmax`) per segment in
  `segments.csv` and in the saved summary.
- Initial Rust workspace and GNOME application skeleton.
- GNU GPLv3-or-later licensing.
- A, C, and Z weighting, time weighting, Leq, sample-peak, and calibration
  DSP primitives.
- A UI-independent engine that continuously computes all live weighting values
  from deterministic mono sample blocks.
- An optional cpal source with PipeWire and PulseAudio support, stable device
  IDs, and lock-free selected-channel delivery.
