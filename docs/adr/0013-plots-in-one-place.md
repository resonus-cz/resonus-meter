# ADR 0013: Plots appear in one place, each with its own axes

## Context

Specification 5.3 asked for one reusable `PlotArea` widget that "Log, RTA and
FFT share". The project owner clarified that this meant the plots appear in
the same place in the window, not that they share axes or one widget.

## Decision

The Log, RTA, FFT and Waterfall plots are tabs in the same area below the
main reading. Each has its own axes and level range. Plot code stays in
`spl-app/src/plot/` without engine knowledge; drawing helpers are shared
where two plots need the same thing, not by design up front.

## Consequences

The Log and RTA plots stay separate modules. Level-axis behaviour (automatic,
fixed, zoom) is chosen per plot.
