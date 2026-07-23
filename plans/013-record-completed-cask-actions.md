# Plan 013: Record completed cask actions as installation truth

> **Executor instructions**: Replace intent-derived receipts with facts emitted
> by successful mutators. This plan improves mise's own truth model; it does not
> authorize Homebrew metadata. Follow every verification and update the index.
>
> **Drift check (run first)**:
> `rtk git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs`
> If installer function signatures or `.mise-cask.toml` changed, compare this
> plan to live code and stop on semantic mismatch.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: Plan 011
- **Category**: architecture / correctness
- **Planned at**: commit `866916893`, 2026-07-23
- **DONE**: 2026-07-23 — mutators emit `CompletedCaskAction`; journal under
  `<prefix>/var/mise/cask-recovery` before mutation and after each completed
  action, with checked file/parent fsync; final `.mise-cask.toml` derives only
  from completed actions after activation; retained sources use final Caskroom
  paths. Re-audit found unknown receipt schemas accepted, empty legacy fields
  reconstructed from the current API, and pending journals unconsumed.
  Schema validation and historical-only legacy status are fixed. Pending
  journals make status unhealthy; successful retry clears same-token journals
  only after durable receipt. Action target type/digest fingerprints detect
  replacement. Unit, lint, and real macOS reinstall gates pass.

## Why this matters

`CaskArtifacts` describes what the API requested. `write_receipt` projects that
intent before all activation steps complete, so a crash or skipped action can
leave a receipt claiming targets that were never successfully installed. Every
future ownership, recovery, uninstall, or handoff decision needs an immutable
manifest of actions mise actually completed.

## Current state

- `CaskArtifacts` contains declarative apps, binaries, packages, fonts, and
  package IDs parsed from the cask definition.
- `install_one` performs app/pkg/font staging and hooks, writes
  `.mise-cask.toml`, renames the version directory, then links binaries/fonts
  and removes stale targets/versions.
- `write_receipt` derives app/binary/font targets and package IDs directly from
  `CaskArtifacts`; it does not consume installer return values.
- `CaskReceipt` records only version and target lists. It lacks action kind,
  source, prior-owner disposition, digest/type, phase, and transaction ID.
- Package installation and hooks can mutate outside Caskroom and need explicit
  rollback/unsupported semantics; their success must not be inferred.

## Commands you will need

| Purpose         | Command                                                                                                                                                     | Expected on success      |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| Locate mutators | `rtk rg -n -e "fn install_" -e "fn stage_" -e "fn link_" -e "fn remove_" -e write_receipt -e CaskArtifacts -e CaskReceipt src/system/packages/brew/cask.rs` | every mutator classified |
| Focused tests   | `rtk cargo test system::packages::brew::cask`                                                                                                               | all pass                 |
| Lint            | `rtk mise run lint-fix`                                                                                                                                     | exit 0                   |
| Diff            | `rtk git diff --check`                                                                                                                                      | no output                |

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- unit tests in that file
- receipt schema/versioning and migration behavior
- fault injection around cask mutation phases
- `plans/README.md` status only

**Out of scope**:

- Homebrew private metadata;
- claiming a distributed transaction with Homebrew;
- executing zap or arbitrary Ruby uninstall blocks;
- reconstructing completed actions from a current API response;
- semver ordering;
- silent migration of receipts whose historical facts are unknowable.

## Git workflow

- Branch: `refactor/brew-cask-completed-actions`
- Commit: `refactor(brew-cask): record completed actions`
- Use `git commit -s` and include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Define the authoritative manifest and phases

Add a versioned `CompletedCaskActionManifest` distinct from `CaskArtifacts`.
Each action must include stable action ID, kind, retained source when relevant,
activated target, operation (`copy`, `move`, `symlink`, package install, hook),
file type/digest where meaningful, result phase, and whether mise created,
replaced, or merely observed the target. Include opaque cask version,
transaction ID, selected platform/architecture, and receipt schema version.

Unknown action/schema versions fail closed. Do not claim that a preexisting
foreign target was created by mise.

**Verify**: serialization round trips; unknown schema/action tests fail with an
actionable error; versions such as `latest` remain opaque strings.

### Step 2: Make mutators return facts

Refactor `install_app`, `install_pkg`, `stage_font`/`link_font`,
`stage_binary`/`link_binary`, and hook execution wrappers so a successful call
returns the exact completed action or appends it to a transaction recorder only
after its linearization point. The caller must not manufacture success records
from `CaskArtifacts`.

For actions with external side effects (`pkg`, hooks), define explicit
`CompletedButNonRollbackable` or ineligible behavior. Do not pretend filesystem
rollback covers package receipts or arbitrary scripts.

**Verify**: fault-injection tests at each mutator boundary show failed/unreached
actions absent from the completed list and prior successful actions retained.

### Step 3: Journal before publishing the final receipt

Persist transaction phase and completed actions durably as mutation advances.
Use temp-file write, file sync, atomic rename, and parent-directory sync. Keep
the recovery journal in a prefix-owned same-volume recovery root outside
`Caskroom/<token>` and `.metadata`, because Homebrew cleanup controls those
directories. Publish the final `.mise-cask.toml` only after every required
activation succeeds; then mark/clean the journal idempotently.

Never write the final receipt before binary/font activation and obsolete-target
cleanup complete. A crash must yield `Pending` with a journal, not a healthy
mise-owned receipt.

**Verify**: kill/fault tests at every phase deterministically resume or roll
back; no final receipt exists for an incomplete transaction.

### Step 4: Make status consume recorded truth

Change installed detection and status to validate completed recorded actions,
not re-derive expected targets from the live cask API. Separate dimensions:

- payload presence/health;
- ownership/mutation authority;
- convergence to requested version;
- transaction/handoff phase.

A missing target is degraded/conflict, not package absence. A valid foreign
takeover is `Externalized`; it is not repairable from current API.

**Verify**: table-driven tests cover absent, healthy mise-owned, pending,
degraded, externalized, and conflicting state without network access.

### Step 5: Define legacy receipt handling

Read the old schema only for mise-only status/uninstall facts it actually
contains. Mark it `LegacyUnverified` for interop/handoff. Never upgrade it to a
completed-action manifest by refetching the same version: cask definitions can
change without a version change. Offer reinstall or explicit read-only
diagnosis instead.

**Verify**: a legacy receipt plus changed same-version fixture never becomes
handoff-eligible automatically.

## Test plan

- Manifest schema round-trip and unknown-version rejection.
- One success and injected failure after every mutator/phase.
- Preexisting foreign target versus mise-created target provenance.
- Crash before and after final receipt rename and parent sync.
- Status dimensions with network unavailable and API definition changed.
- Legacy receipt remains usable only within its proven information boundary.
- Package/hook non-rollbackable cases fail eligibility explicitly.

## Done criteria

- [ ] Declarative `CaskArtifacts` and completed actions are separate types.
- [ ] Only mutators emit completed-action facts.
- [ ] Final receipt is published after all required activations.
- [ ] Journal durability includes file and parent-directory sync.
- [ ] Status does not use current API to invent historical actions.
- [ ] Legacy receipts cannot become interop/handoff eligible automatically.
- [ ] Focused tests and lint pass.

## STOP conditions

Stop if a mutator's success point cannot be identified; if package/hook rollback
would require unsafe deletion; if journal storage is under a Homebrew-controlled
token directory; if status still needs live API to establish installed truth;
or if implementing this requires silently trusting old receipts.

## Maintenance notes

This manifest is mise's authority, not Homebrew's tab schema. Keep it semantic
and versioned. A future supported Homebrew handoff may consume a validated
projection, but private Homebrew field names must never shape this core model.
