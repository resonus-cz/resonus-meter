# Measurement file format

This document defines version 1.2 of the `splmeter-session` portable
measurement format. The canonical representation is a UTF-8 folder:

```text
<measurement>/
  measurement.md
  log.csv
  markers.csv
  segments.csv
  rta_log.csv        (optional, band levels)
  rta_segments.csv   (optional, band levels)
  attachments/       (optional)
```

`measurement.md` is Markdown with YAML front matter. Parsers use its
`format`, `version`, and `files` entries to locate the CSV files; readers must
not assume their paths. Writers set `format: splmeter-session` and
`version: "1.2"`, a quoted `"major.minor"` string; files written before 1.1
carry the integer `1`, meaning 1.0. Readers compare the major part only:
they tolerate unknown front-matter keys, CSV columns and marker kinds, but
reject a higher major version.

The front matter records the measurement name, start/end UTC timestamps,
duration, software, log interval, 20 µPa reference pressure, capture device,
calibration, optional author/tags, and the paths under `files`. It is followed
by user Markdown and a fenced `yaml {#summary}` block. The block holds the
saved range, A/C/Z `{ Leq_db, peak_db, Fmax_db }` values, and the aggregate `clip`
flag. Timestamp values are UTC ISO 8601 with milliseconds.

## Band levels (RTA log)

A session that recorded band levels (the **Record band levels** RTA
preference) adds an `rta` block to the front matter, two files under
`files:` and a list to the summary:

```yaml
rta:
  quantity: LZeq
  bands_per_octave: 3
  bands_hz: [20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500]
files:
  …
  rta_log: rta_log.csv
  rta_segments: rta_segments.csv
```

and, in the summary block, `rta_LZeq_db: [41.20, 43.05, …]`, one value per
band of `bands_hz` (`null` for a band without samples). The quantity is
always the Z-weighted (unweighted) band equivalent level; apply A or C
weighting afterwards per band if needed. `bands_per_octave` is 1 (octave)
or 3 (one-third octave); `bands_hz` lists the nominal mid-band frequencies
(IEC 61260-1 base-ten bands) of the analysed range below the input's Nyquist
frequency. A bundle without band levels has none of these entries.

```csv
t_s,utc,LZeq_500,LZeq_1000
1.000,2026-10-16T09:12:04.250Z,38.50,35.25
```

`rta_log.csv` has one row per `log.csv` row, with the same `t_s` and `utc`.
Each band column holds the band `LZeq` over the measured time since the
previous row: normally one log interval, after a log-interval change the
time since the last row, and after a pause the measured time before the
pause plus the time since `resume`. Columns are named `LZeq_` plus the
nominal frequency in Hz (`LZeq_31.5`, `LZeq_1000`). A blank cell means the
band had no samples.

```csv
segment,start_marker,end_marker,start_s,end_s,duration_s,LZeq_500,LZeq_1000
1,1,2,0.000,412.000,412.000,38.50,35.25
```

`rta_segments.csv` has one row per row of `segments.csv` with the band `LZeq`
of that segment; a pause leaves the band cells blank. Segments combine as in
`segments.csv`, per band. The band filters run continuously, so a segment's
first moments include some decaying energy from before its start marker, as
for `LXFmax`.

## CSV files

All CSV files are UTF-8, LF-delimited, comma-separated, have exactly one
header row, and contain no comments or blank lines. Decimal values use `.`.
Times (`t_s`, ranges, durations) have three decimal places; levels use two.

```csv
t_s,utc,marker_id,clip,LAF,LCF,LZF
1.000,2026-10-16T09:12:04.250Z,,0,41.05,49.00,54.90
```

`log.csv` contains the Fast A/C/Z values at every interval end. `t_s` derives
from sample count, not wall clock. `marker_id` references `markers.csv` when
a marker falls in the interval. `clip` is `0` or `1`.

```csv
id,t_s,utc,kind,label,note
1,0.000,2026-10-16T09:12:03.250Z,start,Start,
2,412.000,2026-10-16T09:18:55.250Z,user,AC on,"Chiller switched on"
```

`markers.csv` kinds are `start`, `stop`, `user`, `reset`,
`interval_change`, `pause` and `resume`. Every marker closes the previous
segment and opens the next. `start` and `stop` occur only at the very
beginning and end of a recording.

```csv
segment,start_marker,end_marker,start_s,end_s,duration_s,clip,LAeq,LApeak,LCeq,LCpeak,LZeq,LZpeak,LAFmax,LCFmax,LZFmax
1,1,2,0.000,412.000,412.000,0,38.70,60.20,47.10,64.00,52.30,66.50,49.80,55.30,59.10
```

`segments.csv` records final A/C/Z values for each closed segment. Peak is
the maximum weighted **sample peak**, not true peak. `Leq` is the energy mean.
`LXFmax` is the largest Fast time-weighted level reached within the segment;
the Fast detector runs continuously, so a segment's first few hundred
milliseconds still carry some decaying energy from before its start marker.
The `LXFmax` columns follow the 1.0 columns.
To combine segments, use

`10 * log10(sum(duration_s * 10^(Leq / 10)) / sum(duration_s))`

and take the maximum segment peak and the maximum segment `LXFmax`. Every
recorded sample lies in exactly one non-empty segment, so `duration_s` is
proportional to its sample count; the app weights by the exact count, so a
recombination from the rounded file values may differ by about 0.01 dB.
Empty segments, such as a pause, write blank final-value fields and do not
contribute to a combined result.

## Example reader

[`docs/examples/read_session.py`](examples/read_session.py) is an
independent reader that uses the Python standard library only. It follows
the `files:` block, rejects a higher major version, recombines the segment
`Leq` and prints a JSON summary:

```sh
python3 docs/examples/read_session.py path/to/measurement-folder
```

A test in `spl-engine` saves a bundle with a marker and a pause and reads it
back with this script; it is skipped when `python3` is not installed.

## Attachments

Files the user adds are copied into `attachments/`; a name used twice gets
`-2`, `-3`, … before its extension. Each is listed under `files:` with its
path relative to the bundle, a title and the lower-case hexadecimal SHA-256
of the copied file:

```yaml
files:
  log: log.csv
  markers: markers.csv
  segments: segments.csv
  attachments:
    - file: "attachments/room_photo.jpg"
      title: "room_photo"
      sha256: "3b7d…"
```

The `attachments` key is omitted when there are none. Attachments are copied
before `measurement.md` is written, so a listed file always exists.

## Saving a range

A bundle may hold only the part of a session between two markers. It then
contains the segments from the start marker to the end marker, those markers,
and the `log.csv` and `rta_log.csv` rows inside the range; the summary
`range` and values, including `rta_LZeq_db`, cover that part. Times keep their session values, so `t_s` of the first row is not
zero and `utc = start_utc + t_s` still holds; `end_utc` is the end marker's
time.

## Recording behaviour

Pausing writes a `pause` marker and resuming a `resume` marker. While paused,
no `log.csv` rows are written and no samples enter the final values, but
session time keeps running: `t_s` counts the stream's samples including the
paused ones, so `utc` after a pause is still true wall-clock time. The segment
between `pause` and `resume` therefore has the pause's duration and blank
final values, and contributes nothing to combined values. After `resume` the
log interval grid restarts at the resume time. Reset ends a pause.

The engine maintains these segment values separately from display state.
Resetting clears the current recording history, including its prior log rows,
markers, and segments. The fresh history begins with a `reset` marker at
`t_s = 0` and retains the previously selected log interval. A save creates a completed snapshot with a `stop` marker while live
recording may continue, so saving does not interrupt the input pipeline.

Changing the active log interval likewise writes an `interval_change` marker,
closes the current segment, and schedules subsequent Fast A/C/Z rows at the
new interval. User markers created from the Log view use `user`; their
timestamp is the instant **Add Marker** was clicked, even when the label is
submitted later.

When an approved calibrator capture is saved, the display and recording history
reset. The fresh history starts with a `reset` marker and uses the approved
calibration; previously recorded uncalibrated rows are discarded.

## Writer guarantees

The session writer creates the canonical CSV files and writes
`measurement.md` last. It writes the Markdown to a sibling temporary file,
fsyncs it, then atomically renames it to `measurement.md`; a completed
Markdown file therefore never refers to unwritten CSV files.

## Version history

- **1.2:** optional band levels: the `rta` front-matter block, `rta_log.csv`,
  `rta_segments.csv` and `rta_LZeq_db` in the summary. A marker whose label
  is entered later is noted in the first `log.csv` row after its time, like
  any other marker.

- **1.1:** `pause` and `resume` marker kinds; `LAFmax`, `LCFmax` and
  `LZFmax` columns in `segments.csv` and `Fmax_db` in the summary;
  `version` written as the string `"1.1"`.
- **1.0:** initial format (written as `version: 1`).
