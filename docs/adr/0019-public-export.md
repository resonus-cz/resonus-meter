# 0019: Public export at release tags

Date: 2026-11-06

## Status

Accepted

## Context

The app is developed in a private repository that also holds the owner's
workflow files (task specifications, notes, reviews, standards). The released
code should be public under its licence (AGPL-3.0-only), the same way as the
other Re:sonus apps.

## Decision

- At each release tag, `scripts/export_public.sh <tag> <public-clone>` copies
  only the paths listed in `scripts/public_files.txt` (an allowlist) into a clone
  of `resonus-meter-public`, which is mirrored to GitHub (`resonus-cz`).
- The export runs only from a clean checkout of the tag after `just check`
  passes; the exported tree is checked for local paths, internal files and
  internal references, its tests are run, and it is committed as
  "Release <tag>" and tagged with the same tag.
- Contributions arriving on GitHub are integrated in the private repository
  and credited with a `Co-authored-by:` trailer in the next public release.

## Consequences

- The public history has one commit per release, not the development history.
- A new top-level file is private until it is added to the allowlist.
