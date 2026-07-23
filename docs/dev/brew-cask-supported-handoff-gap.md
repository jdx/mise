# Supported Homebrew cask handoff gap

**Status:** local decision package; no upstream contact authorized or made  
**Evidence:** Plan 012, GitHub Actions run `29979380126`  
**Pinned Homebrew:** `6bd951d96e7ebc54787799dba77bfb26ec956c4c`

## Current boundary

| Surface                                              | Status                                                                  |
| ---------------------------------------------------- | ----------------------------------------------------------------------- |
| `brew install --cask --adopt`                        | supported full install; app-focused; no external-payload equality proof |
| Homebrew cask installer/tab APIs                     | private Ruby implementation                                             |
| machine-readable handoff validation/dry-run          | absent                                                                  |
| staged-payload ownership transfer with atomic result | absent                                                                  |

Mise keeps direct pours mise-owned and invisible to Homebrew. Native Homebrew
install is supported when the user selects Homebrew as manager from the start.
No receipt-only registration API is requested: Plan 012 first proves a smaller
gap.

## Smallest missing capability

Extend native cask install/adopt with a machine-readable validation operation
that performs no mutation and returns:

- resolved token, opaque version, artifact classes, targets, and dependencies;
- whether each existing target is adoptable;
- whether byte/type equality was actually verified;
- unsupported phases and precise rejection reasons;
- a versioned validation ID/digest usable by the subsequent install.

The mutation operation would accept that validation ID, acquire Homebrew's own
lock, revalidate it, and return a structured atomic outcome. Success makes
Homebrew sole owner. Rejection leaves every target, dependency, Caskroom path,
and marker unchanged.

## Required semantics

- Homebrew owns validation, locking, ledger creation, cleanup, and schema
  migration.
- Existing targets are never removed before all artifacts and dependencies can
  commit.
- Input paths are canonicalized; symlink traversal and target substitution fail.
- No manifest field executes shell or Ruby.
- Validation IDs bind token, definition, platform, targets, digests, and
  dependency plan; stale/replayed IDs fail.
- Retry is idempotent. Concurrent callers get one winner or structured conflict.
- The ledger becomes visible only at the operation's documented linearization
  point; crash before it restores the complete prior filesystem state.
- Pkg, zap, and flight blocks remain unsupported until independently modeled.

## Exact motivating failures

- Different app bytes were adopted successfully, so `--adopt` did not prove
  payload identity.
- Mixed app/binary adoption failed after deleting the existing app.
- Dependency-bearing binary adoption failed while leaving its formula installed.
- Existing binary targets cannot be adopted; success after removing the target
  is native replacement, not handoff.

Until a supported operation satisfies these cases, mise exposes no handoff,
never writes private Homebrew metadata, and refuses to mutate any token with a
Homebrew marker. Plan 002 remains separately blocked on explicit operator
authorization. This document is not authorization to contact Homebrew.
