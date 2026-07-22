# ADR: brew-cask ownership and Homebrew interop

**Status:** Accepted (eighth-pass normative)  
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
5. **Handoff is opt-in and unproven.** `brew install --cask --adopt` remains a
   candidate for explicit one-way transfer only after disposable isolation
   matrix passes. Until then: mise-only; no production transfer code.
6. **Plan 007 rejected.** No dual-writer transactional upgrades without a
   shared supported lock.
7. **Plan 002 (A3 private experiment)** stays default-off and requires explicit
   operator authorization.

## Support matrix

| Mode | Install | Status | Upgrade | Uninstall | brew list/upgrade |
| ---- | ------- | ------ | ------- | --------- | ----------------- |
| MiseOwned (default) | Rust pour | mise receipt | mise | mise (payload) | unsupported |
| HomebrewOwned | native brew | brew ledger | brew | brew | supported |
| Explicit handoff | not shipped | — | — | — | after proven 012 only |
| A3 private metadata | not shipped | — | — | — | experiment only |

## Consequences

- Tools that assume "binary under prefix ⇒ brew owns it" will still fail on
  mise-owned casks; that is intentional until a proven handoff exists.
- Plans 001/003/004/006/008 remain blocked until ownership/handoff dimensions
  are chosen with disposable evidence.
- Upstream registration (Plan 009) is narrowed: only request gaps left after a
  future successful Plan 012.

## Supersedes

Historical A / A2 / A3 "ship synthetic metadata" recommendations and empty-tab
writer docs. Keep them as research evidence only.
