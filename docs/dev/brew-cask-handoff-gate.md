# brew-cask native handoff gate (Plan 012)

## Status: IN PROGRESS — disposable probe ready, evidence pending

The implementation host remains forbidden. A dedicated manual workflow now
uses a fresh GitHub-hosted `macos-15` VM, verifies
`runner.environment == github-hosted` inside both workflow and E2E harness,
generates deterministic local tap/archive fixtures, and requires an
executed-scenario sentinel. GitHub-hosted canonical Homebrew/Application paths
are disposable with the VM; identical paths on any other runner remain
forbidden.

The initial probe records same-version Caskroom collision behavior and exact
payload digests. It does **not** prove handoff support. Remaining matrix rows
must be implemented, executed, and classified before this gate changes.

## Decision

| Outcome                              | Selected                                   |
| ------------------------------------ | ------------------------------------------ |
| Proven class-limited handoff         | no                                         |
| Native reinstall only                | not productized                            |
| **Unsupported — mise-only retained** | **current safe baseline pending evidence** |

Production code must not expose opt-in transfer until a disposable runner
executes the full fixture matrix (app identical/different, auto_updates,
binary, dependency, pkg/hooks ineligible) with sentinel and collision/failure
rows.

## Remaining unblockers

- Execute the manual workflow and retain its evidence artifact.
- Complete no-Caskroom, differing-target, failure/retry, and Homebrew lifecycle
  rows for every fixture class.
- Add deterministic mixed app/binary and formula-dependency fixtures.
- Add explicit pre-mutation ineligibility checks for pkg and lifecycle hooks.
- Verify post-handoff mise status is `Externalized` and refuses mutation; this
  depends on the revised ownership model in Plan 001.
- Pin the tested Homebrew commit and classify every row.
- Select exactly one Plan 012 decision-gate outcome.

## Safe baseline (shipped)

- Mise-owned pours: no Homebrew `.metadata` (Plan 010)
- Path containment before I/O (Plan 011)
- Completed-action journal + post-activation receipt (Plan 013)
- Foreign Homebrew metadata preserved
