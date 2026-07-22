# Plan 008: Give every cask ownership state a safe recovery path

**2026-07-23 supersession note**: Base diagnosis on Plan 013's recorded facts
and Plan 012's handoff phases. Add a healthy, explicit withdrawal path from
`Externalized` ownership; recovery is not only repair of broken synthetic
metadata. Never reconstruct historical authority from the current API.

> **Executor instructions**: Complete Plans 001, 006, 002, and 007 first. Recovery
> crosses ownership boundaries; default to read-only diagnosis and require an
> explicit strategy plus confirmation for every move. Never delete unknown
> Homebrew metadata. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/cli/system src/system/packages/brew/cask.rs src/system/packages/brew/cask_ownership.rs src/system/packages/brew/cask_transaction.rs`
> Confirm all eight Plan 001 ownership states and Plan 007 transaction phases
> exist. Mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/007-transactional-interop-upgrades.md`
- **Category**: security / dx / architecture
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Fail-closed classification prevents corruption but can strand users in legacy,
orphan, externalized, conflict, or interrupted states. Current plan text points
to a nonexistent future adoption command. Manual deletion is not an acceptable
recovery contract for shared package-manager state.

Recovery must be explicit, reversible where possible, and state-specific. No
command may convert valid Homebrew ownership into mise ownership automatically.

## Current state

- Package command surface under `src/cli/system/` has status, apply, use,
  import, prune, and upgrade; no repair/adopt/reinstall operation.
- Current branch-created metadata can be recognized only heuristically by
  `{}` installed JSON, empty uninstall artifacts, and `(mise)` text. Those
  fields are not sufficient implicit ownership proof.
- Plan 001 adds `PayloadOrphan`, `HomebrewOwned`, `Externalized`, and `Conflict`.
- Plan 007 adds durable transaction phases but requires an operator path when
  external mutation prevents automatic recovery.
- Homebrew maintainers have previously required reinstall to populate missing
  tap receipts because historical ownership cannot be inferred:
  <https://github.com/Homebrew/brew/issues/17416>.

## Commands you will need

| Purpose         | Command                                       | Expected on success           |
| --------------- | --------------------------------------------- | ----------------------------- |
| CLI tests       | `rtk cargo test bootstrap_packages_repair`    | all state/strategy cases pass |
| Cask tests      | `rtk cargo test system::packages::brew::cask` | exit 0                        |
| Render CLI/docs | `rtk mise run render`                         | exit 0                        |
| Lint            | `rtk mise run lint`                           | exit 0                        |
| Diff check      | `rtk git diff --check`                        | no output                     |

If Cargo is not on PATH, use
`rtk proxy /Users/donbeave/.cargo/bin/cargo` with identical arguments.

## Scope

**In scope**:

- new `mise bootstrap packages repair` command under `src/cli/system/`
- brew-cask ownership/recovery APIs
- reversible recovery bundles under a same-volume prefix-owned recovery root
- generated CLI docs/completions
- unit tests and disposable macOS E2E extensions

**Out of scope**:

- Automatic adoption of valid Homebrew-owned payload/metadata.
- Guessing historical artifacts from current API.
- Deleting applications/packages/fonts or running arbitrary uninstall hooks.
- Repairing formula receipts.
- Enabling special cask forms without their explicit gates below.

## Git workflow

- Branch: `advisor/008-cask-interop-recovery`
- Commit: `feat(brew-cask): add explicit interop recovery`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Add read-only diagnosis and a state-to-action matrix

Add:

```text
mise bootstrap packages repair --manager brew-cask [TOKEN...]
```

With no strategy flag, command is read-only. Print token, opaque versions,
ownership state, exact mismatched paths, current transaction phase, and allowed
next actions. Do not print full receipt contents or user paths unrelated to the
cask.

Required matrix:

| State                | Apply           | Mise upgrade            | Read-only repair recommendation                           |
| -------------------- | --------------- | ----------------------- | --------------------------------------------------------- |
| `Absent`             | install         | install current         | none                                                      |
| `PayloadOrphan`      | block           | block                   | quarantine after operator verifies targets                |
| `MiseOnly`           | no-op           | mise upgrade            | enable interop only on next exact capture                 |
| `MiseInteropPending` | phase recovery  | phase recovery          | resume/rollback Plan 007 transaction                      |
| `MiseInterop`        | no-op/reconcile | Plan 007 upgrade        | either manager, never concurrently                        |
| `HomebrewOwned`      | preserve        | fail with owner message | use Homebrew; uninstall there before returning to mise    |
| `Externalized`       | preserve        | fail with owner message | treat as Homebrew handoff or quarantine stale mise ledger |
| `Conflict`           | block           | block                   | quarantine only with explicit strategy                    |

**Verify**: snapshot CLI tests cover every row and prove diagnosis causes zero
filesystem writes.

### Step 2: Create reversible recovery bundles

For Caskroom state, create a bundle under
`<homebrew-prefix>/var/mise/cask-recovery/<id>` containing:

- schema, recovery ID, timestamp, token, classified source state;
- original absolute paths constrained to the token directory;
- entry types and SHA-256 digests;
- paths moved into that recovery directory;
- intended post-state.

Use same-filesystem renames and verify device identity before mutation. Never
copy then delete. Bundle itself is written and synced before the first move. A
small index may live in normal mise state, but it is not recovery authority.
Never leave mise-private entries at Caskroom token root or `.metadata` root;
Homebrew uninstall/doctor treats those leftovers as corruption. A
`--restore <recovery-id>` operation restores only when live destinations are
absent and quarantine digests match.

Do not move application/font/pkg payload targets as generic “conflict repair.”
Quarantine only manager ledgers and exact transaction-owned state; preserve
unexplained payload/targets and report them for class-specific recovery.

**Verify**: fault tests at every bundle/move point either restore original state
or leave a complete restorable bundle; modified quarantine refuses restore.

### Step 3: Implement narrow explicit strategies

Add mutually exclusive strategies requiring `--yes` (and dry-run support):

- `--resume-transaction`: use only Plan 007 journal rules.
- `--rollback-transaction`: use exact old manifests only.
- `--quarantine-legacy-interop`: for a valid mise receipt plus the known branch
  synthetic shape, move `.metadata` intact to recovery storage, yielding
  `MiseOnly`. The shape is an eligibility check, not ownership proof; explicit
  confirmation supplies authority.
- `--quarantine-conflict`: move only selected ledger paths listed in the
  diagnostic bundle; never generic payload/targets. Resulting state may be
  `MiseOnly` or `PayloadOrphan`, never assumed healthy.
- `--retire-stale-mise-ledger`: when state is `Externalized`, quarantine only
  the stale mise receipt/provenance and leave valid Homebrew state untouched,
  yielding `HomebrewOwned`.

Do not add `--adopt-homebrew`. Transfer from `HomebrewOwned` back to mise is:
Homebrew exact uninstall first, verify `Absent`, then normal mise apply. If
Homebrew metadata is invalid, quarantine/restore workflow must resolve it before
any manager reinstalls.

**Verify**: state × strategy table rejects every invalid combination before
mutation and proves valid Homebrew metadata is byte-identical unless the chosen
strategy explicitly retires only mise state.

### Step 4: Define legacy backfill policy

Legacy `.mise-cask.toml` lacks raw effective API data, staged-source mapping,
dependency provenance, and Homebrew tree hashes. Matching only current version
string is insufficient because cask definitions can change without a version
bump. Therefore:

- never auto-backfill legacy metadata from current API;
- `--quarantine-legacy-interop` removes unsafe visibility but preserves payload;
- next actual mise upgrade/reinstall captures a new exact snapshot;
- if no version change is available, provide an explicit mise reinstall option
  in this command that downloads, verifies, and stages current payload through
  normal cask transaction primitives, then uses Plan 002 fresh publication;
  never relabel existing files.

The reinstall option is allowed only from `MiseOnly`, uses current actual
actions, and remains subject to Plan 002 eligibility and experimental setting.

**Verify**: same-version but changed-artifact fixture cannot backfill; explicit
reinstall produces new receipt/snapshot through normal transaction code.

### Step 5: Gate token/tap migrations and special versions

Extend raw API parsing with `old_tokens`, effective tap, `variations`,
`url_specs`, `version`, `sha256`, container, dependencies, and conflicts.

- Renamed token or tap migration: remain ineligible until an explicit migration
  transaction moves payload, targets, both ledgers, and config key. Add a
  design-only diagnostic describing source/destination; never follow current
  migration API silently.
- `url_specs.only_path`: enable only when projected staged source exactly
  round-trips through Homebrew loader and real uninstall/rollback tests.
- `version: latest` and `sha256: no_check`: remain ineligible until a disposable
  actual upgrade/reinstall E2E proves version detection and source integrity
  policy. Treat strings as opaque.
- Unknown platform variation/container/conflict shape: fail closed before pour
  or metadata emission, depending on whether it affects payload safety.

**Verify**: fixtures for old token, tap migration, `latest`, `no_check`,
`only_path`, platform variations, container, and conflicts each produce a named
supported/ineligible result—never a generic ignore.

### Step 6: Define ownership behavior after Homebrew takeover

Implement the state/CLI behavior with synthetic valid Homebrew-rewrite fixtures;
Plan 003 then reuses these assertions in its real upgrade fixture:

1. Homebrew upgrades an exact mise interop v1 to v2.
2. Classifier yields `HomebrewOwned` or `Externalized`, depending on whether
   Homebrew retained the old mise receipt.
3. Normal apply preserves and succeeds.
4. `mise bootstrap packages upgrade --manager brew-cask` fails before mutation
   with exact owner/recovery text.
5. Read-only repair recommends Homebrew management.
6. `--retire-stale-mise-ledger` works only for `Externalized` and leaves the
   Homebrew tree byte-identical.

**Verify**: unit/CLI fixtures pass all six assertions without real Homebrew.
Plan 003 is the integration gate on a disposable macOS job.

## Test plan

- Read-only output/state matrix and dry-run.
- Strategy authorization matrix; `--yes` required.
- Recovery bundle fault injection and restore integrity.
- Known legacy synthetic metadata, altered lookalike, invalid JSON, orphan
  payload, valid external migration, interrupted transaction.
- No live API backfill even when version strings match.
- Token/tap/special-version named gates.
- Real Homebrew takeover followed by apply, mise upgrade, repair, and retire.

## Done criteria

- [ ] Every ownership state has exact apply/upgrade/recovery behavior.
- [ ] No output recommends manual deletion of shared metadata.
- [ ] Every mutating repair is explicit, confirmed, and recoverable.
- [ ] Legacy current-API backfill is impossible.
- [ ] Mise upgrade after Homebrew takeover is defined and tested.
- [ ] Token/tap/special-version cases fail closed with named reasons.
- [ ] `rtk cargo test bootstrap_packages_repair` exits 0.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] `rtk mise run render` exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- a strategy needs deleting rather than quarantining unknown metadata;
- the prefix recovery root is not on the same filesystem as moved Caskroom
  state;
- current Homebrew state is structurally invalid but a strategy assumes its
  uninstall is safe;
- adoption requires inferring ownership from `(mise)`, token, version, or path;
- reinstall cannot reuse normal cask staging plus Plan 002 fresh publication;
- renamed token/tap migration would require editing user config implicitly.

## Maintenance notes

- Recovery bundles need documented retention/cleanup policy; never purge an
  unrestored bundle automatically.
- Add a strategy only with a complete source-state predicate, destination state,
  rollback, dry-run, and negative authorization tests.
- Homebrew-owned state is a valid terminal state. Mise config may describe the
  desired package while delegating lifecycle ownership to Homebrew.
