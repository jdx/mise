# Plan 007: Make mise interop upgrades recoverable across payload and both ledgers

**REJECTED AS CURRENTLY DESIGNED (2026-07-23)**: Do not execute this private
dual-writer transaction plan. Homebrew install/upgrade/uninstall does not honor
a lock mise can acquire, so local journaling cannot make two managers atomic.
Re-plan only around one-way native ownership handoff or a future supported
Homebrew protocol with locking/CAS. Recovery journals belong outside
Homebrew-controlled Caskroom token/version/metadata directories.

> **Executor instructions**: Complete Plans 001, 006, and 002 first. Follow
> every phase and fault-injection gate. This plan handles mise-driven upgrades
> of an existing exact interop cask; it does not make concurrent Homebrew and
> mise mutation supported. Update this plan's row in `plans/README.md` when
> done.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs src/system/packages/brew/cask_ownership.rs src/system/packages/brew/cask_metadata.rs src/lock_file.rs`
> Plans 001/002/006 are expected to change cask modules. Confirm their live
> ownership enum, projected snapshot, dependency ledger, and experimental gate
> match this plan. Mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/001-model-cask-ownership.md`,
  `plans/006-cask-dependencies.md`,
  `plans/002-emit-exact-binary-metadata.md`
- **Category**: bug / security / architecture
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Current payload mutation finishes before Homebrew metadata writing starts. A
crash can therefore leave new payload and links under old Homebrew lifecycle
authority. The earlier metadata-only pending protocol has the same defect and
cannot represent new mise receipt plus old live Homebrew metadata without
calling it a conflict.

Upgrade must be one recoverable state machine spanning staged payload, target
links, mise receipt, Homebrew visibility marker, and cleanup. Metadata is
published last. Existing Homebrew authority is revoked before first live
payload mutation.

## Current state

- `src/system/packages/brew/cask.rs:151-188` commits payload/version receipt,
  replaces links, removes stale versions, then writes `.metadata`.
- `src/system/packages/brew/cask.rs:1229-1250` treats multiple version
  directories as unreconciled and ignores only names beginning
  `.mise-tmp-`.
- `src/lock_file.rs:16-45` supplies a mise cache lock. Homebrew does not honor
  that lock; no supported cross-manager lock contract exists.
- Plan 001 defines fresh/upgrade pending journals and full-tree provenance.
- Plan 002 produces current minimal installed JSON plus an exact projected tab.
- Homebrew's cask metadata surface is internal:
  <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/caskroom.rb#L7-L11>.

Repo conventions:

- return `eyre::Result`; use `WrapErr`/`bail!` with actionable context;
- version strings are opaque and compare only with equality;
- all transaction-owned temp names begin `.mise-tmp-`;
- tests may inject failures through private test helpers, never environment
  hooks available in release builds.

## Commands you will need

| Purpose           | Command                                       | Expected on success        |
| ----------------- | --------------------------------------------- | -------------------------- |
| Transaction tests | `rtk cargo test cask_interop_transaction`     | all phase/fault cases pass |
| Cask tests        | `rtk cargo test system::packages::brew::cask` | exit 0                     |
| Lint              | `rtk mise run lint`                           | exit 0                     |
| Diff check        | `rtk git diff --check`                        | no output                  |

If Cargo is not on this workstation PATH, use
`rtk cargo` with identical arguments.

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- `src/system/packages/brew/cask_transaction.rs` (create)
- Plan 001 ownership and Plan 002 metadata modules
- unit/fault-injection fixtures

**Out of scope**:

- App, font, pkg, hook, or completion payload transactions.
- Automatic recovery of foreign/conflicting/legacy metadata; Plan 008 owns UX.
- Claiming mutual exclusion with Homebrew.
- Real Homebrew lifecycle E2E; Plan 003 owns disposable-machine tests.
- Enabling interop by default.

## Git workflow

- Branch: `advisor/007-transactional-cask-interop-upgrade`
- Commit: `fix(brew-cask): make interop upgrades recoverable`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Persist a phase-aware transaction journal

Create an additive, checksummed journal under the old installed version,
`Caskroom/<token>/<old-version>/.mise-tmp-interop-<id>/transaction.json`:

```rust
enum TransactionKind { Fresh, Upgrade } // shared Plan 001 type
enum TransactionPhase {
    Prepared,
    HomebrewVisibilityRevoked,
    PayloadSwitching { completed_targets: Vec<PathBuf> },
    PayloadSwitched,
    MetadataPublished,
}

struct CaskTransactionJournal {
    schema: u32,
    id: String,
    token: String,
    kind: TransactionKind,
    phase: TransactionPhase,
    old: Option<CaskStateManifest>,
    new: CaskStateManifest,
}
```

This plan writes only `Upgrade`; Plan 002 uses `Fresh` and locates its journal
inside the newly installed version directory.

The same transaction directory contains prepared payload and old metadata
backup. Unknown files under version directories are purged safely by Homebrew;
mise-private entries at token root or `.metadata` root can block removal and
must never be created.

Each state manifest includes exact opaque version, payload/version tree digest,
mise receipt digest, target path/type/link destination, and complete Homebrew
metadata tree digest. The payload-tree manifest excludes exactly its own named
transaction directory to avoid a self-referential digest; every other unknown
entry is included. The journal separately hashes every staged/backup entry.
Journal paths are normalized and must remain under the token directory or the
exact permitted target list.

Write journal atomically and sync file plus parent directory before advancing a
phase. Never infer phase from path existence alone.

**Verify**: `rtk cargo test cask_transaction_journal` -> old/new round-trip,
path traversal rejection, corrupted checksum rejection, and every phase parse.

### Step 2: Prepare all replacement state before revoking visibility

Under the mise token lock:

1. Re-read ownership; require exact `MiseInterop` and enabled experimental
   setting.
2. Download/extract new payload into the journal's version-local
   `new-payload` directory.
3. Build new projected metadata and mise receipt from actual staged files.
4. Validate dependencies, source/target links, JSON, full-tree manifests, and
   rollback inputs.
5. Persist `Prepared` journal containing exact old and new manifests.

No live payload, target, receipt, or `.metadata` path changes before the
`Prepared` journal is durable.

**Verify**: injected failure during preparation leaves old payload/links/both
ledgers byte-identical; retry discards only matching transaction-owned staging.

### Step 3: Revoke old Homebrew authority before payload mutation

Re-read old fingerprints. Atomically rename the complete live `.metadata` tree
to the journal's version-local `old-metadata`, then durably advance phase to
`HomebrewVisibilityRevoked` before changing payload or links.

After rename, new Homebrew commands should report the cask not installed. A
Homebrew process that loaded metadata before the rename can still race; the
mise cache lock cannot prevent it. Re-check all old payload/target fingerprints
before every following phase. Any unexplained mutation becomes `Externalized`
or `Conflict`; do not restore or overwrite external bytes.

**Verify**: phase test proves marker disappears in one rename; simulated
external target or backup mutation aborts without overwriting it.

### Step 4: Switch binary payload and links with per-target recovery

Keep old version directory until final cleanup.

1. Rename prepared new payload into its final version directory.
2. For each binary target, verify it is the exact old symlink recorded in the
   journal. Create a sibling new symlink and rename it over the target.
3. After each target rename, atomically advance `completed_targets`.
4. Write new `.mise-cask.toml` carrying new snapshot/provenance intent.
5. Advance phase to `PayloadSwitched` only after every target and receipt match
   the new manifest.

If any target is a regular file, foreign link, or changed symlink, stop. Roll
back only targets still matching the transaction's new manifest; never delete
an unexplained target.

**Verify**: fault at every target index recovers old links and old version;
multiple-binary test never deletes foreign replacement created after a fault.

### Step 5: Publish exact metadata as linearization point

Publish Plan 002's prevalidated metadata tree with one rename. Immediately
verify the live tree manifest and advance to `MetadataPublished`. No live
payload or target mutation occurs after this point; Homebrew may now act on the
new exact state.

Only after `MetadataPublished` may cleanup remove old version and transaction
backup paths whose digests still match the journal. Cleanup failure leaves a
valid new install plus recoverable transaction debris; it must not revoke the
new marker.

**Verify**: process-death simulation immediately before/after publication
recovers deterministically; published state is always payload/receipt/metadata
version-consistent.

### Step 6: Implement phase-aware restart recovery

At start of every brew-cask status/apply/upgrade mutation, detect a journal and
classify:

- `Prepared`: discard matching new staging; old live state stays authoritative.
- `HomebrewVisibilityRevoked`: restore old metadata if every old payload/target
  fingerprint still matches; otherwise preserve and report conflict.
- `PayloadSwitching`: roll back completed matching targets and new version, then
  restore old metadata; refuse if any path changed externally.
- `PayloadSwitched`: either publish already-prepared exact new metadata or roll
  back using old manifests. Choose roll-forward only when every new payload and
  target digest matches.
- `MetadataPublished`: verify new state, then finish digest-guarded cleanup.

Dry-run reports phase and proposed recovery without mutation.

**Verify**: exhaustive table test covers every phase × expected/missing/foreign
path combination; no branch guesses from current API.

## Test plan

- Journal serialization, checksum, traversal, symlink, and schema rejection.
- Fault/process-death injection before and after every durable phase.
- One and multiple renamed binaries; opaque/non-semver versions.
- External mutation at old metadata, old/new version, each target, and receipt.
- Retry/idempotency after each recoverable state.
- Published metadata always loads without API fallback in Plan 002 fixtures.
- No test writes `/opt/homebrew`; use temp prefix/unit fixtures here.
- Root-layout fixture proves no transaction file appears directly under token
  root or `.metadata` root. Plan 003 verifies Homebrew uninstall/doctor after
  simulated cleanup failure.

## Done criteria

- [ ] Durable intent exists before first live payload mutation.
- [ ] Old Homebrew marker is hidden before link/version changes.
- [ ] Every crash phase has one tested roll-forward/rollback rule.
- [ ] External mutation is preserved and blocks mise overwrite.
- [ ] No post-publication payload mutation occurs.
- [ ] Interop remains experimental/default-off.
- [ ] `rtk cargo test cask_interop_transaction` exits 0.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- new payload cannot be fully staged while old payload remains usable;
- rollback needs a source not retained in old payload/journal;
- a target cannot be replaced/restored without deleting an unexplained path;
- Homebrew has a live marker during any payload mutation;
- current Homebrew behavior requires a mutually honored lock to be safe;
- a non-binary artifact is needed to pass the transaction tests;
- Plan 001 cannot classify an observed crash phase without guessing.

## Maintenance notes

- Atomic visibility reduces races; it does not create cross-manager locking.
  Default-off experimental status remains mandatory without an upstream
  ownership/registration contract.
- Review ordering before adding each artifact class. App/pkg/hooks need their
  own reversible payload transaction, not reuse of binary assumptions.
- Homebrew activity after metadata publication transfers authority when it
  changes provenance; Plan 008 defines user-visible recovery/transfer choices.
