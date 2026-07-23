# ADR: brew-cask ownership and Homebrew interop

**Status:** Accepted (Plan 012 final)  
**Date:** 2026-07-23  
**Related:** `HOMEBREW_FINDINGS.md`, `plans/README.md`, Plans 010–013

## Context

Homebrew's installed cask marker (`.metadata` + tab) is **lifecycle authority**,
not identity. Writing synthetic empty-tab metadata made mise claim authority it
could not describe and allowed Homebrew to recover teardown from the live API.

## Decision

1. **Default: MiseOwned.** Direct `brew-cask:` pours write only mise receipts
   (completed-action schema v2). No Homebrew `.metadata`.
2. **Preserve foreign Homebrew metadata.** Never delete/rewrite genuine brew
   ledgers during cleanup.
3. **Path safety first.** Untrusted token/version/artifact paths are validated
   centrally before any I/O.
4. **Completed actions, not intent.** Mutators record durable facts; final
   receipt publishes after activation; journals live outside Caskroom/metadata.
5. **Handoff is unsupported.** Disposable Plan 012 evidence proves
   `brew install --cask --adopt` lacks equality validation and safe,
   observable rollback for mise payloads. No production transfer code.
6. **Homebrew markers block mise mutation.** Mise may observe a healthy
   same-version Homebrew cask, but refuses upgrade/recovery mutation across
   versions. Homebrew remains sole mutator.
7. **Plan 007 rejected.** No dual-writer transactional upgrades without a
   shared supported lock.
8. **Plan 002 (A3 private experiment)** stays default-off and requires explicit
   operator authorization.

## Support matrix

| Mode                | Install     | Status       | Upgrade | Uninstall      | brew list/upgrade |
| ------------------- | ----------- | ------------ | ------- | -------------- | ----------------- |
| MiseOwned (default) | Rust pour   | mise receipt | mise    | mise (payload) | unsupported       |
| HomebrewOwned       | native brew | brew ledger  | brew    | brew           | supported         |
| Explicit handoff    | unsupported | —            | —       | —              | unsupported       |
| A3 private metadata | not shipped | —            | —       | —              | experiment only   |

## Consequences

- Tools that assume "binary under prefix ⇒ brew owns it" will still fail on
  mise-owned casks; that is intentional until a proven handoff exists.
- Plan 001's applicable boundary is complete: marker ownership and mutation
  authority remain separate; foreign markers block mise mutation.
- Plans 003/004/006/008 are closed as not applicable to the rejected handoff.
- Plan 009 records the smallest missing public capability locally; no upstream
  contact occurred.

## Supersedes

Historical A / A2 / A3 "ship synthetic metadata" recommendations and empty-tab
writer docs. Keep them as research evidence only.
