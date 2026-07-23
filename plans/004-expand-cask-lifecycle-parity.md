# Plan 004: Expand Homebrew interoperability one lifecycle class at a time

**2026-07-23 supersession note**: Promotion means native handoff eligibility
unless an explicit private-experiment decision says otherwise. Do not infer
parity from mise install support. Every class still needs install, upgrade,
reinstall, uninstall, rollback, offline, foreign-target, and failure E2E.

> **Executor instructions**: Complete Plans 001, 006, 002, 007, 008, and 003 first. This is a gated
> expansion plan, not permission to mark every cask eligible. Each artifact
> class becomes eligible only after its own layout, snapshot, rollback, and
> real-Homebrew tests pass. Update `plans/README.md` after each completed gate.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs src/system/packages/brew/cask_metadata.rs e2e/cli/test_system_install_brew_macos_slow`
> Confirm the ownership state machine, exact dependency/snapshot adapters,
> transactions, recovery, and apply-time reconciliation exist. Absence is a
> STOP condition.

## Status

**CLOSED — NOT APPLICABLE after Plan 012.** No artifact class passed the
handoff safety gate. App, auto-update app, binary, mixed, dependency, pkg, and
flight-hook rows are classified in `docs/dev/brew-cask-handoff-gate.md`.
Artifact support in mise remains distinct from Homebrew lifecycle parity.

- **Priority**: P2
- **Effort**: L per artifact class
- **Risk**: HIGH
- **Depends on**: `plans/003-reconcile-and-test-cask-interop.md`
- **Category**: feature / architecture / tests
- **Planned at**: commit `866916893`, 2026-07-23

> **Research update 2026-07-23**: the app-class mismatch is harder than
> "copy vs move". Brew's Moved artifact leaves a **symlink** at the staged
> Caskroom source (`moved.rb:171-175`); on uninstall/upgrade `move_back`
> raises "It seems there is already an App at '<source>'" when a real
> directory sits there (`moved.rb:198-204`). Mise stages a real copy, so
> non-forced brew lifecycle on a mise-poured app cask _errors out_ even with
> exact metadata — Step 3 must replicate the symlink layout, not just choose
> a canonical one. Casks with `uninstall_preflight`/`uninstall_postflight`
> receive `.rb` caskfiles from brew itself (`installer.rb:517-523`) and can
> never reach **JSON** parity — ineligible under Plans 002/003, fail closed.
> Current source trace and sandbox evidence identified a plausible future gate
> for this class: install the checksum-verified, version-matched
> tap `.rb` as the caskfile (mise already fetches it — `ruby_source_path`/
> `ruby_source_checksum`); real flight blocks then execute, migration leaves
> the file untouched, and official-tap `.rb` is trusted untapped via
> `tab.source.tap`. Hard conditions: exactly one caskfile per timestamp dir
> (a stale `.json` beats `.rb` in `CASKFILE_EXTENSIONS` priority,
> `caskroom.rb:14,56-58`), filename == token, tab carries `source.tap` +
> `uninstall_flight_blocks: true` + the full artifact list. Treat as a
> Step-4-class gate with its own E2E; not v1. Binary artifacts are Symlinked,
> not Moved — unaffected.

## Why this matters

Mise currently understands app, binary, font, and pkg artifacts, but that does
not mean it reproduces Homebrew's lifecycle. Example: Homebrew moves an app to
the application directory while current mise code copies it and retains the
Caskroom source. Hooks, completions, uninstall blocks, pkg receipts, and zap
add further state. Emitting a Homebrew marker before those states agree gives
Homebrew destructive authority over a layout it did not create.

Correct expansion rule: support is a matrix of artifact class × install ×
upgrade × uninstall × rollback × offline behavior. A green install alone is
insufficient.

## Current state

- `src/system/packages/brew/cask.rs:916-966` parses app, binary, pkg, and font.
- `src/system/packages/brew/cask.rs:406-446` executes lifecycle hooks.
- `src/system/packages/brew/cask.rs:1649-1666` skips completions, manpages,
  uninstall, zap, and flight-block artifacts for payload installation.
- Current official Codex metadata contains generated shell completions and zap
  in addition to its binary:
  <https://formulae.brew.sh/api/cask/codex.json>.
- Homebrew's artifact cookbook defines the manager-specific activation
  behavior this adapter must match:
  <https://docs.brew.sh/Cask-Cookbook>.
- Related branch work must be inspected, not assumed complete: PRs
  [#11197](https://github.com/jdx/mise/pull/11197) and
  [#11198](https://github.com/jdx/mise/pull/11198) were open and unmerged when
  this plan was written.

## Commands you will need

| Purpose         | Command                                                             | Expected on success |
| --------------- | ------------------------------------------------------------------- | ------------------- |
| Cask unit tests | `rtk cargo test system::packages::brew::cask`                       | exit 0              |
| macOS E2E       | `rtk mise run test:e2e e2e/cli/test_system_install_brew_macos_slow` | CI macOS: exit 0    |
| Full lint       | `rtk mise run lint`                                                 | exit 0              |
| Diff check      | `rtk git diff --check`                                              | no output           |

If `rtk cargo` cannot find Cargo, use
`rtk proxy /Users/donbeave/.cargo/bin/cargo` with identical arguments.

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- exact metadata/ownership/transaction/recovery modules from prerequisite plans
- macOS slow E2E and class-specific fixtures
- dedicated disposable macOS workflow/job from Plan 003
- related open PR changes after explicit diff review

**Out of scope**:

- Blanket eligibility for an untested artifact class.
- Executing arbitrary Ruby to translate Homebrew metadata.
- Claiming compatibility from API shape alone.
- Automatic adoption of Homebrew-owned installs.
- Registry changes or formula backend behavior.

## Git workflow

Use one branch and commit per completed artifact-class gate. Example:
`feat(brew-cask): support exact completion lifecycle`.
Use `git commit -s`; include
`Co-authored-by: Codex <codex@openai.com>`.
Do not push or open a PR unless operator asks.

## Steps

### Step 1: Build a versioned compatibility matrix

Add a test-owned matrix with one row per artifact class:

| Class              | Mise install layout    | Homebrew layout       | Exact snapshot | Upgrade | Uninstall | Rollback | Eligible |
| ------------------ | ---------------------- | --------------------- | -------------- | ------- | --------- | -------- | -------- |
| binary             | Plan 002               | symlink from Caskroom | required       | tested  | tested    | tested   | yes      |
| completion/manpage | measure                | measure               | missing        | missing | missing   | missing  | no       |
| app                | copy + retained source | move/activate         | mismatch       | missing | missing   | missing  | no       |
| font               | measure                | measure               | missing        | missing | missing   | missing  | no       |
| pkg                | installer receipt      | installer receipt     | missing        | missing | missing   | missing  | no       |
| hook/flight block  | partial execution      | Ruby lifecycle        | missing        | missing | missing   | missing  | no       |

Replace `measure` only with behavior observed from current Homebrew source and a
disposable real install. Record the Homebrew commit and macOS version in fixture
documentation. Unknown or mixed classes remain ineligible.

**Verify**: a unit test maps every parsed artifact variant to an explicit
supported/ineligible reason; adding an enum variant without updating the matrix
fails compilation or test.

### Step 2: Add completions and manpages first

Inspect PR #11198 against current main. Reuse only changes that satisfy the
following gates:

1. Actual install target and symlink/copy mode match Homebrew.
2. Generated completion commands are deterministic, sandboxed, and captured in
   the transaction; failed generation rolls back every installed target.
3. Exact tab contains only generated files that exist.
4. Homebrew uninstall removes those files and no unrelated target.
5. Upgrade removes stale completion/manpage names.
6. Offline reinstall succeeds from the captured version data or fails closed
   before mutation; it never silently fetches current API metadata.

Extend eligibility only for casks whose every activatable artifact is now
represented. Codex is the primary mixed binary/completion fixture.

**Verify**: real Homebrew list, upgrade dry-run, uninstall, and induced rollback
pass for Codex-like fixture; stale completion sentinels are handled exactly.

### Step 3: Make app and font layout semantically identical

For each class independently:

1. Compare ownership, copy/move/link semantics, quarantine handling, target
   conflict rules, auto-update behavior, and stale cleanup with Homebrew.
2. Choose one canonical layout. If matching it changes existing mise behavior,
   provide an explicit migration path and rollback test; do not silently move
   user-managed app/font targets.
3. Snapshot source and target paths exactly.
4. Test upgrade from old mise-only layout to the new layout.
5. Test Homebrew uninstall and failed-upgrade rollback on a disposable cask.

App and font eligibility remain separate. A mixed app+binary cask is eligible
only when both rows pass and the combined rollback test passes.

**Verify**: Hidden Bar remains the negative fixture until all app gates pass;
afterward the same E2E becomes positive and proves exact cleanup.

### Step 4: Gate pkg and Ruby lifecycle sources separately

Inspect PR #11197 against current main. Before enabling pkg metadata:

- capture installer package IDs and pre-install state;
- distinguish newly installed receipts from pre-existing shared receipts;
- implement exact uninstall/upgrade semantics without removing packages owned
  by another cask or user;
- model supported non-Ruby lifecycle steps as explicit actions with rollback
  boundaries;
- prove interrupted install and failed uninstall recovery.

Never infer package ownership from `pkgutil` presence alone. Never serialize an
uninstall action mise cannot safely reproduce or Homebrew cannot safely consume.

Run pkg lifecycle tests only on a disposable VM image. A temporary `HOME` does
not isolate the system package receipt database. The cleanup assertion must
prove every test-owned receipt, target, cache, and Caskroom path is gone.

**Verify**: disposable signed/unsigned pkg fixtures cover install, upgrade,
uninstall, shared receipt conflict, hook failure, and rollback. All foreign
sentinels remain byte-identical and the VM is discarded.

Treat uninstall Ruby flight blocks as a separate `.rb` caskfile gate, never as
JSON stubs:

1. Independently reproduce Homebrew's `.rb` install/load/uninstall behavior on
   a disposable fixture and pin the source commit.
2. Require exact fetched Ruby bytes bound to the effective token, opaque
   version, tap, `ruby_source_path`, and `ruby_source_checksum`. Apply the same
   tap trust policy Homebrew would use. Missing/mismatched/untrusted source is
   ineligible.
3. Publish exactly one caskfile in the timestamp `Casks/` directory, named for
   the token. A stale `.json` must not coexist because Homebrew extension
   priority would select it first.
4. Set tab `source.tap`, `source.version`,
   `uninstall_flight_blocks: true`, and the complete exact uninstall artifact
   list. Capture the `.rb` bytes in provenance/tree hashes.
5. Prove Homebrew update leaves the `.rb` usable, untapped source resolution
   works through the recorded tap, real flight blocks execute exactly once,
   and rollback/restoration preserves byte identity.

This gate authorizes future Homebrew execution of Ruby. Review it as code trust,
not metadata parsing. It remains ineligible if exact behavior requires any
unverified source or if mise did not execute/record corresponding install-time
lifecycle behavior.

**Verify**: disposable `.rb` fixture passes list, info, actual upgrade,
uninstall, rollback, untapped-source, checksum mismatch, stale-JSON, and
untrusted-tap cases.

### Step 5: Add continuous drift and downgrade gates

For every newly eligible class:

- keep canonical fixtures from the pinned Homebrew commit;
- test the exact Homebrew commit used by golden fixtures and current stable
  Homebrew public CLI behavior on macOS CI;
- define an explicit supported-contract matrix before claiming more than one
  Homebrew version; do not invent an “oldest supported” release that mise has
  never declared or tested;
- fail closed on unknown tab/config/artifact schema;
- emit one actionable incompatibility message, not partial metadata;
- document how to disable only Homebrew interop while leaving mise payload
  management usable.

When upstream changes invalidate a row, downgrade future writes for that row
before releasing and offer Plan 008 de-registration for existing metadata.
Code drift cannot retroactively protect markers already emitted. All private
receipt publication therefore stays experimental/default-off unless Plan 009
lands a supported upstream contract.

**Verify**: deliberately unknown artifact/schema fixture produces no Homebrew
marker and retains a working mise-owned payload.

## Test plan

- Exhaustive artifact-variant-to-eligibility mapping.
- Golden exact snapshots per supported class and mixed-class combination.
- Fresh install, same-version reinstall, version upgrade, downgrade/ref tag,
  uninstall, zap, and injected failure at each mutation boundary.
- Online and offline behavior; live API may never substitute for installed
  version state.
- Foreign/pre-existing target conflicts and byte-preservation sentinels.
- Real Homebrew behavior for every class promoted to eligible.
- Opaque version strings; no semver sorting or “newest” inference.

## Done criteria

- [ ] Compatibility matrix names every parsed artifact class.
- [ ] Each eligible row passes install/upgrade/uninstall/rollback E2E.
- [ ] Mixed casks require all component rows and a combined transaction test.
- [ ] Unsupported or unverified hooks/unknown artifacts fail closed before
      metadata emission.
- [ ] Pinned and current-stable Homebrew contract tests pass; every additional
      claimed version is named in the tested compatibility matrix.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] macOS CI E2E exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report for the affected class if:

- exact behavior requires executing untrusted Ruby;
- Homebrew and mise layouts cannot be made semantically identical without an
  explicit user migration;
- uninstall ownership cannot distinguish shared/pre-existing state;
- rollback cannot restore both payload and metadata ledgers;
- real Homebrew loads current API instead of the installed snapshot;
- an open PR implements only the happy-path install;
- a test would touch a pre-existing user application, font, package, or cask.

## Maintenance notes

- This plan is intentionally incremental. Binary-only interoperability is a
  valid release boundary; unsafe broader visibility is not.
- Plan 009's supported external-registration contract is the robust release
  path. Until then, the isolated private adapter remains experimental and
  default-off even when its class gates pass.
- Every newly supported class enlarges destructive authority. Review it as a
  security boundary, not only a feature.
