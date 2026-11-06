# ADR 0017: Re:sonus window layout and plot style

## Context

The Re:sonus apps share one window layout (stack app architecture, section 2,
decision D8: header brand, ribbon, tabbed panel; a deliberate departure from the
GNOME HIG decided by the project owner for resonus-room on 2026-11-05) and one plot
style (resonus-core style tokens, decision D-T1, light and dark variants). The
Python apps get both from resonus-core and the app template (`gui/layout.py`); the
meter is Rust and cannot import them. The owner asked for the meter to follow the
template (2026-11-05, review item accepted with "yes to all"; title "Meter").

## Decision

- **Layout** (`spl-app/src/layout.rs`, a port of the template's `gui/layout.py`
  with the same CSS):
  - header bar: brand "Re:sonus" left (Lexend semibold), `AdwWindowTitle` "Meter"
    centred with the input as subtitle, the main menu right; no other controls;
  - ribbon: Measure (Pause, Reset, Add Marker), Reading (weighting and quantity
    toggles, moved out of the meter surface to fit the window height), Device
    (Input… and Calibrate… open those preferences pages; calibration status),
    Files (Save…, New Session). One vertical `GtkSizeGroup` lines the captions
    up; a narrow window scrolls the ribbon horizontally;
  - the reading stays the central surface; below it Log and RTA are an
    `AdwViewStack` with a wide `AdwViewSwitcher` over an accent-framed panel; below
    400 sp an `AdwBreakpoint` hides the switcher and reveals an
    `AdwViewSwitcherBar` at the bottom (the template states no width; 400 sp is
    where the wide switcher plus margins stops fitting);
  - window title "Re:sonus Meter" (window list), default size 940 × 860 so the
    whole ribbon is visible.
- **Plot style** (`spl-app/src/style.rs`, `plot/text.rs`): the resonus-core
  tokens file `resonus_core/assets/style_tokens.toml` is vendored as
  `spl-app/data/style_tokens.toml` and compiled in (`include_str!`). It is pinned
  to a resonus-core tag (`TOKENS_SOURCE`, now `v0.10.0`; was commit `a4cc7af`); a test
  compares the copy with `git show <pin>:<path>` in a sibling resonus-core
  checkout and is skipped when there is none. Never edit the copy.
  Light or dark follows `AdwStyleManager:dark`; the plots redraw when it changes.
  Traces A, C, Z use trace colours 1-3, RTA bars colour 1, marker lines and the
  peak-hold line the neutral colour, text the axes colour. Sizes in points are
  converted at 96 px per inch (Pango-Cairo's default resolution).
- **Text** through Pango-Cairo layouts (not the Cairo toy text API) in the token
  font. New dependencies: `pango` 0.21 (feature `v1_56` for
  `FontMap::add_font_file`) and `pangocairo` 0.21, from the gtk-rs project (MIT,
  already in the tree through gtk4). Pango >= 1.56 is needed at run time
  (Fedora 44 has 1.57).
- **Fonts**: Lexend Regular, Medium and SemiBold (SIL OFL 1.1, source in
  `data/fonts/lexend/SOURCE.md`, the same files as resonus-core) are compiled in,
  written to `$XDG_CACHE_HOME/cz.resonus.SplMeter/fonts` at startup and added to
  the Pango font map. If that fails, text falls back to the default font.
- **Plot mark**: the token brand text ("Re:sonus"), semibold, brand colour, at
  the bottom right corner of each plot (as resonus-core `style.add_brand`). The
  replaceable mark of resonus-room (its D-24, a user setting) is not yet in the
  meter: it needs a settings field and a preferences row (planned).
- **`--screenshot=PATH`** (and `--screenshot-delay=SECONDS`, default 2) renders
  the window to a PNG and quits; used for the README screenshots and layout
  checks. Not documented in `--help`; development use.

## Consequences

- The meter looks like the other Re:sonus apps; header-bar controls moved into
  the ribbon, so the README text and muscle memory change.
- A style change in resonus-core reaches the meter only when the copy and pin
  are updated; the test catches a copy that drifted from its pin, not a pin
  that is behind core main.
- If the layout helpers move to resonus-core (stack question Q1/Q-C6), this
  module stays the Rust counterpart and keeps the CSS identical by hand.
