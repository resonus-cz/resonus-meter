# ADR 0016: Re:sonus identity, licence and repository

## Context

The meter joins the Re:sonus stack of acoustic apps (review of
2026-11-05, items I1-I4, accepted by the project owner on
2026-11-05). The stack uses `cz.resonus.<Name>` application ids (stack decision
D7, the owner holds the domain), AGPL-3.0-only (stack decision D-25) and a
private Gitea repository with a published subset mirrored to the GitHub
organisation `resonus-cz` (stack decision D-27). The app has not been released,
so its id can still change.

## Decision

- Application id `cz.resonus.SplMeter` (was `io.github.vyhyb.SPLMeter`). At
  startup, before anything is read, the config folder
  (`$XDG_CONFIG_HOME/<id>`: settings, calibration history) and the state folder
  (`$XDG_STATE_HOME/<id>`: session journals) of the former id are renamed to
  the new id, once, if the new folder does not exist yet. A failed rename is
  reported on stderr; the old folder is never deleted or overwritten
  (`spl-app/src/migrate.rs`).
- Licence AGPL-3.0-only (was GPL-3.0-or-later). This relicensing assumes the
  project owner holds the copyright of all code in the repository (all
  commits are the owner's or made on the owner's behalf; no third-party code
  was copied, contributor guidelines section 10). `deny.toml` allows the dependency
  licences under AGPL-3.0-only; `cargo deny check` passes.
- Repository: `resonus-cz/resonus-meter` on the private Gitea, published
  subset and GitHub mirror as the other Re:sonus repositories (the public
  export is set up before the first release).
- Workflow: this file's ADRs, branches and Conventional Commits stay. Added:
  `docs/COMPLIANCE.md` (requirement -> code -> test, with edition and
  clause), standards cited from the owner's library (never committed).
- Unchanged for now: the window title (the Re:sonus layout follows in its own
  change).

## Consequences

- The Flatpak/AppImage data files, when they are added, use the new id.
- A user of a development build keeps settings, calibrations and journals.
