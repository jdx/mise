# Plan 003: Reconcile exact metadata during apply and test real Homebrew lifecycle

**2026-07-23 supersession note**: Plan 012 now owns native handoff feasibility
and its disposable Homebrew lifecycle matrix. Run this plan only after the
selected ownership direction and Plan 008 recovery behavior are finalized.
A suite needs an executed-scenario sentinel; filtered zero-test or whole-script
early exit is failure, not success.

> **Executor instructions**: Execute Plans 001, 006, 002, 007, and 008 first. Follow this plan
> in order. Real Homebrew lifecycle tests are CI-only and may remove their own
> disposable test casks. Stop on any STOP condition.
>
> **Research correction 2026-07-23**: current API version equality is not
> historical proof. Cask definitions can change without a version bump, and
> target existence does not prove skipped artifacts, source paths, hooks, or
> dependency closure. Never backfill legacy lifecycle authority from live API.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/mod.rs src/cli/system/driver.rs src/system/packages/brew/cask.rs e2e/run_test e2e/cli/test_system_install_brew_macos_slow .github/workflows`
> Prerequisite plans should have changed cask modules. Verify ownership,
> dependency, snapshot, transaction, and recovery APIs exist.

## Status

**CLOSED — NOT APPLICABLE after Plan 012.** Mise-owned casks make no Homebrew
list/upgrade/uninstall claim, and no handoff state ships. The disposable matrix
instead proves native Homebrew lifecycle only after a successful native
install/adopt and records why that cannot be exposed as preserving transfer.

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/008-cask-interop-recovery.md`
- **Category**: bug / tests / dx
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Current repair lives inside `install_one`, but normal apply filters
`PackageState::Installed` before calling the manager. Re-running bootstrap does
not repair missing metadata despite docs saying it converges. Calling the
package missing would be a status lie.

Reconciliation must be a separate manager hook: payload status remains
truthful, while apply may repair self-authored exact compatibility state.
Real-Homebrew tests must cover list, upgrade loading, uninstall cleanup, and
repair reachability; helper-only unit tests cannot prove that boundary.

## Current state

- Trait contract says `installed` is side-effect free:

  ```rust
  // src/system/packages/mod.rs:100-104
  async fn installed(&self, pkgs: &[PackageRequest])
      -> Result<Vec<PackageStatus>>;
  async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts)
      -> Result<()>;
  ```

- Driver excludes installed packages during apply, includes them during
  upgrade:

  ```rust
  // src/cli/system/driver.rs:91-100
  .filter(|s| match action {
      Action::Install => !matches!(s.state, PackageState::Installed { .. }),
      Action::Upgrade => !matches!(s.state, PackageState::Missing),
  })
  ```

- Repair currently exists only in `install_one` at
  `src/system/packages/brew/cask.rs:111-126`.
- Existing macOS slow test installs Hidden Bar and checks only mise status and
  filesystem paths at `e2e/cli/test_system_install_brew_macos_slow:23-33`; it
  never runs `brew`.
- E2E convention: invoke through
  `mise run test:e2e <test-file>`, never execute scripts directly.

## Commands you will need

| Purpose           | Command                                                             | Expected on success                         |
| ----------------- | ------------------------------------------------------------------- | ------------------------------------------- |
| Driver/unit tests | `rtk cargo test cli::system::driver`                                | exit 0                                      |
| Cask unit tests   | `rtk cargo test system::packages::brew::cask`                       | exit 0                                      |
| macOS E2E         | `rtk mise run test:e2e e2e/cli/test_system_install_brew_macos_slow` | CI macOS: all assertions pass; non-CI: skip |
| Lint              | `rtk mise run lint`                                                 | exit 0                                      |
| Diff check        | `rtk git diff --check`                                              | no output                                   |

If `rtk cargo` cannot find Cargo on this workstation, use
`rtk cargo` with identical arguments.

## Scope

**In scope**:

- `src/system/packages/mod.rs`
- `src/cli/system/driver.rs`
- `src/system/packages/brew/cask.rs`
- prerequisite metadata/ownership/transaction/recovery modules
- `e2e/cli/test_system_install_brew_macos_slow`
- `e2e/run_test` and the dedicated macOS workflow/job
- focused driver unit tests

**Out of scope**:

- Changing `PackageState` meaning.
- Repairing legacy receipts without exact snapshots.
- Making app/pkg/font casks Homebrew-visible.
- Running destructive Homebrew tests on developer machines.
- New CLI subcommands or settings.

## Git workflow

- Branch: `advisor/003-reconcile-cask-interop`
- Commit: `test(brew-cask): verify exact Homebrew interop`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Add a default no-op reconciliation hook

Add to `SystemPackageManager`:

```rust
async fn reconcile_installed(
    &self,
    _pkgs: &[PackageRequest],
    _opts: &InstallOpts,
) -> Result<()> {
    Ok(())
}
```

Name may vary, but semantics are fixed:

- called only for packages already classified `Installed`;
- may repair manager-owned metadata, never payload;
- honors `dry_run`;
- default no-op preserves every other manager;
- must not elevate unless explicit apply already permits it.

Call it in driver after status query and before target filtering for
`Action::Install`. Do not call it from status/doctor.

**Verify**: `rtk cargo test reconcile_installed` -> fake manager proves apply
calls hook for installed packages, upgrade does not double-call it, dry-run is
passed, and missing packages still use `install`.

### Step 2: Reconcile only exact self-authored snapshots

Override hook in `BrewCaskManager`.

- `MiseInteropPending`: dispatch by journal kind. `Fresh` follows Plan 002;
  `Upgrade` follows Plan 007. Regenerate nothing from live API; resume or roll
  back only from frozen manifests and staged files.
- `MiseInterop` is already converged. If either live hash is corrupt/missing,
  classify `Conflict`; do not call corruption a repairable pending transaction.
- `MiseOnly` with an exact, immutable Plan 002 snapshot may enter a fresh
  interop transaction only under explicit opt-in. A legacy receipt is not such
  a snapshot.
- Legacy v1 receipt: no backfill from current API. Log one actionable message;
  wait for a real version upgrade/reinstall to capture exact data.
- `HomebrewOwned`, `Externalized`, `PayloadOrphan`, and `Conflict`: preserve and
  report; never synthesize or overwrite lifecycle authority.
- Ineligible cask: no-op without repeated noisy warnings.

Dry-run prints an exact action such as
`repair self-authored Homebrew metadata token/version`; it does no writes.

**Verify**: `rtk cargo test reconcile_homebrew_cask_metadata` -> state table
covers every ownership/receipt version, exact pending roll-forward, partial-tree
conflict, and dry-run.

### Step 3: Make the real-Homebrew E2E impossible to silently skip

The normal E2E harness constructs an isolated environment without forwarding
`CI`; a script guard on `CI=true` can therefore make a green job run zero
Homebrew assertions. Fix the harness or dedicated job to explicitly pass
`CI=${CI:-}`. Add an execution counter: when the job declares the Homebrew
suite required, zero executed scenarios is a failure.

Split every pre-existing-state check into a per-scenario skip. Hidden Bar
already being present must not `exit` the whole script and suppress later
binary/dependency scenarios. Print executed/skipped counts at the end.

Do not run lifecycle mutation in the normal shared macOS job. Add a dedicated
disposable macOS runner/job because the harness isolates `HOME` but not the
global Homebrew prefix, `/Applications`, or the pkg receipt database. Use
unique test tokens, tap names, targets, and cache paths; maintain one cleanup
stack covering HTTP process, tap, target, Caskroom tree, cache, and temp data.
Assert the stack completed. Plan 004 pkg cases require a disposable VM image,
not merely a fresh home directory.

**Verify**: a harness test intentionally omits `CI` and proves the required job
fails for zero scenarios; a pre-existing Hidden Bar fixture skips only that
scenario.

### Step 4: Prove list, reconciliation, and uninstall with a local binary fixture

Use a deterministic localhost archive/API fixture and unique local tap rather
than a mutable public cask for lifecycle assertions. A public cask may remain a
non-blocking smoke test only.

1. Assert unique token paths/targets absent; skip rather than touch foreign
   state.
2. Install the fixture with interop disabled. Assert `MiseOnly`, an immutable
   pour-time exact snapshot, no Homebrew marker, and record payload/download
   fingerprints.
3. Enable interop and run normal apply. Prove the installed-package
   reconciliation hook publishes from the saved snapshot without payload
   download or mutation.
4. Assert provenance, tab, and installed caskfile exist.
5. Assert tab `uninstall_artifacts` is non-empty, matches actual binary actions,
   and excludes a fixture completion mise deliberately did not install.
6. Run `brew list --cask --versions <token>`; assert the exact opaque version.
7. Run `brew upgrade --cask --dry-run <token>`; assert no “not installed” or
   metadata-recovery failure.
8. Separate unit/fault fixtures cover exact pending roll-forward and prove
   deleting metadata from converged `MiseInterop` classifies `Conflict`. Do not
   damage the live E2E fixture to manufacture this state.
9. Seed transaction-owned cleanup debris only inside the version directory,
   then run `brew uninstall --cask <token>`. Assert exact targets, Caskroom, and
   metadata are removed, unrelated sentinels survive, and `brew doctor` reports
   no corrupt leftover token root.

Use existing assertion helpers. Do not add executable bits or execute the
script directly.

**Verify**: required CI exits 0; its execution counter proves the lifecycle
scenario ran, while focused unit tests prove the pending/conflict branches.

### Step 5: Prove a real v1 to v2 Homebrew upgrade and ownership transition

Extend the same fully disposable third-party tap fixture:

1. Start a localhost server with a v1 binary archive and exact API response.
   Route only test URLs through `MISE_URL_REPLACEMENTS`; add no production API
   override.
2. Create a uniquely named local Homebrew tap with v2 cask/archive. Refuse to
   run if its tap, Caskroom, binary target, or metadata already exists.
3. Pour v1 through mise. Assert exact non-empty v1 receipt/provenance.
4. Run `brew upgrade --cask <token>` against the local tap.
5. Assert v2 payload/metadata, v1 target removal, and no public current-API
   recovery request.
6. Classify the normal takeover as `HomebrewOwned` when Homebrew purges the old
   version containing the mise receipt. Repeat with an injected retained stale
   receipt and require `Externalized`, not `Conflict`. Normal mise status/apply
   preserves both valid outcomes. Explicit mise upgrade fails before mutation
   and points to Plan 008 recovery.
7. Run Homebrew uninstall; assert exact cleanup and preserved sentinels.

The cleanup stack may remove only unique resources it created. It must not use
broad Homebrew cleanup or touch a user tap.

**Verify**: macOS CI performs actual v1→v2 upgrade; HTTP log proves installed-v1
teardown did not substitute public current API metadata.

### Step 6: Add a negative ineligible-layout E2E

Retain Hidden Bar app scenario, but align expectation with safe eligibility:

- mise install/status works;
- no Homebrew installed caskfile is emitted until app layout parity exists;
- `brew list --cask --versions hiddenbar` fails as not installed;
- no foreign metadata is touched.

This test prevents future blanket metadata emission from bypassing eligibility.

**Verify**: macOS E2E passes both eligible binary and ineligible app scenarios.

### Step 7: Add Homebrew contract drift detection

Unit fixtures from Plan 002 must record at least:

- Homebrew main commit `c010c96b` behavior;
- immediately previous installed-JSON contract fixture when available;
- expected authoritative non-empty tab behavior;
- expected missing-artifact fallback behavior.

Add a CI assertion using installed Homebrew that reads generated metadata and
loads it through public CLI behavior. Exercise a normal Homebrew metadata
migration/rewrite and prove the classifier transitions to `Externalized` (or
remains `MiseInterop` when bytes stay unchanged), never `Conflict` solely
because Homebrew normalized valid files. Do not scrape private source line text
as the only test; behavior matters.

When drift fails, disable future publication for that contract version and say
review is required. This cannot make already-published private metadata safe;
interop remains experimental/default-off until Plan 009 provides a supported
upstream contract. Provide a kill switch and Plan 008 de-registration path for
existing markers.

**Verify**: mutate a local fixture schema in a unit test and confirm adapter
rejects it with the expected compatibility error.

## Test plan

- Fake manager hook call matrix: apply/upgrade, installed/missing/mismatch,
  dry-run/real.
- Reconciliation ownership matrix and exact-snapshot requirement.
- Failure injection leaves package status truthful and metadata recoverable.
- Harness propagation, required execution sentinel, and per-scenario skip.
- Real Homebrew list and upgrade dry-run on deterministic local binary.
- Deterministic real Homebrew v1 to v2 upgrade through a local tap, including
  `HomebrewOwned` takeover and stale-receipt `Externalized` variant.
- Real Homebrew uninstall removes only installed binary artifacts.
- Ineligible app never gets synthetic metadata.
- Dedicated CI tests never touch pre-existing user casks or global pkg state
  outside their disposable runner.

## Done criteria

- [ ] Normal apply reaches metadata reconciliation without status lie.
- [ ] Legacy empty receipts are never backfilled from live API.
- [ ] Required CI cannot pass with zero Homebrew scenarios executed.
- [ ] Real Homebrew list, actual v1 to v2 upgrade, and uninstall pass for an
      eligible binary.
- [ ] Ineligible app remains mise-owned and Homebrew-invisible.
- [ ] `rtk cargo test cli::system::driver` exits 0.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] macOS CI E2E exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- E2E would run lifecycle mutation outside a disposable dedicated runner or
  touch a pre-existing cask, application, or global receipt;
- Homebrew uninstall resolves current API despite non-empty exact snapshot;
- actual Homebrew upgrade leaves ambiguous dual ownership rather than a
  classified `HomebrewOwned`/`Externalized` takeover;
- reconciliation requires calling install or mutating payload;
- legacy receipt cannot be distinguished from exact v2 snapshot;
- driver hook changes behavior for another manager;
- Homebrew source/CLI contract drift invalidates Plan 002.

## Maintenance notes

- Keep status side-effect free. Reconciliation belongs only in explicit apply.
- A passing `brew upgrade --dry-run` proves gate/loading, not rollback. Actual
  uninstall test supplies minimum destructive lifecycle evidence.
- Add each newly eligible artifact class to E2E before enabling it in production.
