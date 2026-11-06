# Contributing

- One branch per change from `main` (`feature/…`, `fix/…`, `docs/…`, `chore/…`);
  small commits with Conventional Commit messages (imperative, at most 72
  characters in the subject).
- Measurement code is written test-first against analytic or standards
  references; cite the standard with edition and clause.
- Layers stay clean: the UI does not know DSP, DSP does not know UI, files or
  audio APIs.
- Docs move with code: every user-visible change adds a line under `Unreleased`
  in `CHANGELOG.md`; architectural decisions go to `docs/adr/`.

Install the task runner and dependency checker once:

```sh
cargo install just
cargo install cargo-deny --locked
```

Run `just check` before opening a pull request. It runs `rustfmt`, Clippy,
`cargo deny check` (licences, advisories, sources; configured in `deny.toml`),
and the workspace tests.
