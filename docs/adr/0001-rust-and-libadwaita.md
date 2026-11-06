# ADR 0001: Use Rust and libadwaita

## Context

The application needs trustworthy numerical processing and a GNOME-native
desktop user interface.

## Decision

Use Rust for all layers. The desktop application uses GTK4 and libadwaita;
the DSP and engine layers do not depend on either UI library.

## Consequences

Core code remains suitable for a future non-GTK front end. The Linux build
requires GTK4 and libadwaita development libraries.
