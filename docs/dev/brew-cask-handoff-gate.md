# brew-cask native handoff gate (Plan 012)

## Status: STOP — isolation unproven

Disposable macOS prefix/appdir isolation was **not** available on the
implementation host (real `/opt/homebrew` + `/Applications`). Per plan STOP
conditions, **no** adopt/install/uninstall experiments ran against operator
state.

## Decision

| Outcome | Selected |
| ------- | -------- |
| Proven class-limited handoff | no |
| Native reinstall only | not productized |
| **Unsupported — mise-only retained** | **yes** |

Production code must not expose opt-in transfer until a disposable runner
executes the full fixture matrix (app identical/different, auto_updates,
binary, dependency, pkg/hooks ineligible) with sentinel and collision/failure
rows.

## Safe baseline (shipped)

- Mise-owned pours: no Homebrew `.metadata` (Plan 010)
- Path containment before I/O (Plan 011)
- Completed-action journal + post-activation receipt (Plan 013)
- Foreign Homebrew metadata preserved
