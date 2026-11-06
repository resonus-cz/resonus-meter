# ADR 0012: One recovery prompt for all leftover journals

Amends [ADR 0010](0010-journal-with-recovery.md), which offered only the
newest journal per start.

## Context

Every run that ends without a clean close (a crash, `kill`, or Ctrl+C in a
terminal) leaves a journal. With one offer per start, the prompt returned on
every start until each journal was handled.

## Decision

- Journals without data (nothing recorded and no marker beyond the first)
  are deleted on start without asking.
- One dialog covers the rest. With several sessions it lists them, newest
  selected. **Recover** continues the chosen session and deletes all other
  journals, as the dialog states; **Discard All** deletes every journal,
  including unreadable ones; **Not Now** keeps them all for the next start.
- Unreadable journals are listed in the same dialog with the reason; they
  are deleted with Recover or Discard All.

## Consequences

The prompt appears at most once per start and, after Recover or Discard
All, not again for the same journals. Recovering one session deliberately
gives up the others.
