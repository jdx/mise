# Plan 001: Make cask ownership explicit and prevent implicit Homebrew takeover

**2026-07-23 supersession note**: Do not execute this plan before Plans 011,
012, and 013. Revise its state model to keep payload owner, Homebrew marker
owner, mutation authority, convergence/health, handoff phase, and contract
version as separate dimensions. Native Homebrew handoff ends in
`Externalized`; it must not be folded into synthetic-metadata convergence.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Stop
> on any condition listed under "STOP conditions"; do not improvise. When done,
> update this plan's row in `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs src/cli/system/driver.rs`
> If either file changed, compare the excerpts below with live code. Mismatch is
> a STOP condition.

## Status

**DONE (revised after Plan 012):** no handoff state ships. Payload provenance
remains in the mise receipt; Homebrew marker ownership is detected separately;
mutation authority is fail-closed. Same-version Homebrew state may satisfy
status without mutation. Any older, degraded, malformed, or symlinked Homebrew
marker blocks mise install/upgrade before bootstrap, download, or payload I/O.
This removes the implicit-takeover enabling condition without inventing an
`Externalized` ledger.

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: none
- **Category**: bug / security / architecture
- **Planned at**: commit `866916893`, 2026-07-23

> **Research update 2026-07-23**: source trace confirmed this plan's premises.
> The concrete takeover vector is `installed_cask_version`'s no-receipt branch
> (`cask.rs:1371-1391`): a brew-installed cask counts as installed, so a mise
> upgrade pours over it and the provenance-blind writer (`cask.rs:1441-1457`)
> destroys brew's exact tab (real zoom tab carries `launchctl`/`pkgutil`/
> `delete` stanzas that would be lost). Brew's installed gate globs across all
> versions (`caskroom.rb:47-62`) while the repair probe checks only the
> current version (`cask.rs:1505-1526`) — Step 2's cross-version requirement
> is mandatory, not defensive.

## Why this matters

Current branch treats Homebrew's installed caskfile as an identity marker.
Homebrew treats it as lifecycle authority: upgrade, uninstall, reinstall, and
zap load old artifacts from that metadata. Current writer can also delete
Homebrew-authored version metadata and replace its receipt during a mise
upgrade. This violates stated goal G6 and makes ownership ambiguous.

Correct invariant: only manager proven to own a ledger may mutate it. Presence
of `.mise-cask.toml` proves mise poured payload; it does not prove mise owns an
existing Homebrew `.metadata` tree.

## Current state

- `src/system/packages/brew/cask.rs` owns cask install, receipt, status, and the
  new Homebrew metadata writer.
- `src/cli/system/driver.rs` selects apply/upgrade targets.
- Current successful install always invokes the writer:

  ```rust
  // src/system/packages/brew/cask.rs:171-188
  write_receipt(&tmp_caskroom, &cask, &artifacts)?;
  file::remove_all(&caskroom)?;
  file::rename(&tmp_caskroom, &caskroom)?;
  // ...links and stale cleanup...
  write_homebrew_cask_metadata(&caskroom_token, &cask, &artifacts)?;
  ```

- Writer deletes every top-level metadata version directory, then overwrites
  the shared tab:

  ```rust
  // src/system/packages/brew/cask.rs:1441-1468
  if metadata.is_dir() {
      for entry in std::fs::read_dir(&metadata)?.filter_map(|e| e.ok()) {
          // ...fixed-file exceptions...
          if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
              file::remove_all(entry.path())?;
          }
      }
  }
  crate::file::write(
      metadata.join("INSTALL_RECEIPT.json"),
      serde_json::to_string_pretty(&receipt)?,
  )?;
  ```

- Repair ownership proof is only a matching mise payload receipt:

  ```rust
  // src/system/packages/brew/cask.rs:1496-1503
  let Some(receipt) = read_receipt(&version_dir)? else {
      return Ok(false);
  };
  Ok(receipt.version == cask.version
      && !homebrew_installed_caskfile_exists(&token_dir, cask)?)
  ```

- Existing serialization convention: `CaskReceipt` uses `serde(default)` for
  additive fields, preserving old receipts. Match that convention.
- Existing locking convention: `crate::lock_file::LockFile::new(path).lock()`;
  see `src/lock_file.rs:16-45`.
- Error convention: return `eyre::Result`, add context with `WrapErr`, and use
  `bail!` for actionable ownership conflicts.

External contract, pinned for this plan:

- Homebrew `Cask#installed?` is caskfile existence, but upgrade then loads that
  caskfile and starts uninstalling old artifacts:
  <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/upgrade.rb#L191-L209>
  and
  <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/upgrade.rb#L366-L491>.
- Homebrew documents `Cask::Tab` as private and changeable without warning:
  <https://docs.brew.sh/rubydoc/Cask/Tab>.

## Commands you will need

| Purpose       | Command                                       | Expected on success         |
| ------------- | --------------------------------------------- | --------------------------- |
| Focused tests | `rtk cargo test system::packages::brew::cask` | exit 0; all cask tests pass |
| Driver tests  | `rtk cargo test cli::system::driver`          | exit 0                      |
| Lint          | `rtk mise run lint`                           | exit 0                      |
| Diff check    | `rtk git diff --check`                        | no output                   |

If `rtk cargo` cannot find Cargo in this workstation shell, use
`rtk proxy /Users/donbeave/.cargo/bin/cargo` with identical arguments. This
fallback was required during plan creation.

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- `src/system/packages/brew/cask_ownership.rs` (create if separation makes the
  state machine clearer)
- unit tests in those modules

**Out of scope**:

- `src/cli/system/driver.rs` behavior; reconciliation belongs to Plan 003.
- Generating new Homebrew metadata; Plan 002 owns that.
- Shelling out to `brew install --cask`.
- Registry, formula, import, prune, or unrelated cask artifact support.
- Deleting or auto-migrating legacy synthetic metadata.

## Git workflow

- Branch: `advisor/001-cask-ownership`
- Commit: `fix(brew-cask): prevent implicit Homebrew takeover`
- Commit with `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Add a persisted provenance record

Extend `CaskReceipt` additively with an optional metadata provenance object.
Use a versioned shape, not a boolean:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct HomebrewMetadataProvenance {
    schema: u32,
    token: String,
    version: String,
    installed_caskfile_relpath: PathBuf,
    metadata_tree_sha256: String,
    tab_sha256: String,
    config_sha256: String,
    installed_caskfile_sha256: String,
}

#[serde(default)]
homebrew_metadata: Option<HomebrewMetadataProvenance>,
```

Names may vary, but all fields above are required. The relative path must be
strictly normalized under `.metadata`; reject absolute paths and `..`. Hash a
sorted manifest of every relative path, entry type, and file digest in the
complete tree; reject symlinks and unknown paths. Individual hashes remain for
diagnostics. A `.mise-cask.toml` without this object owns payload only, never
`.metadata`.

Add round-trip tests for old receipts and v2 receipts.

**Verify**: `rtk cargo test cask_receipt` -> old receipt parses with `None`; new
receipt round-trips without field loss.

### Step 2: Implement an ownership classifier

Create an enum with these states:

```rust
enum CaskOwnership {
    Absent,
    PayloadOrphan,
    MiseOnly,
    MiseInteropPending,
    MiseInterop,
    HomebrewOwned,
    Externalized,
    Conflict,
}
```

`MiseInteropPending` carries a persisted transaction kind (`Fresh` or
`Upgrade`) and phase. An upgrade journal contains both previous live
payload/metadata manifests and intended replacement manifests; pending is not
defined merely by an absent `.metadata` tree.

Classification inputs:

- every payload/version directory and relevant target, including payload with
  no ledger;
- current versioned `.mise-cask.toml` and transaction journal;
- any Homebrew installed caskfile using Homebrew's cross-version glob semantics,
  not only `.metadata/<current-version>`;
- `.metadata/INSTALL_RECEIPT.json` and `config.json` bytes;
- the exact installed-caskfile relative path from provenance;
- provenance hashes when present.

Rules:

- `PayloadOrphan`: payload/version directory or managed-looking targets exist,
  but neither valid ledger proves ownership. Never treat this as `Absent`.
- `MiseOnly`: mise receipt exists, no Homebrew installed caskfile, no provenance.
- `MiseInteropPending`: a valid journal explains every observed old/new/staged
  path and hash for its recorded phase. Unexplained partial state is `Conflict`.
- `MiseInterop`: provenance exists and both current Homebrew files match token,
  version, and the complete tree manifest.
- `HomebrewOwned`: valid Homebrew installed state exists with no mise receipt.
- `Externalized`: Homebrew state is structurally valid but no longer matches
  mise provenance. Treat Homebrew as authoritative and preserve both ledgers;
  this covers legitimate Homebrew migration/upgrade that leaves a stale mise
  receipt. Never silently refresh provenance.
- `Conflict`: Homebrew data is invalid/partial/ambiguous, a journal does not
  explain observed paths, versions disagree without valid external state, or
  multiple installed markers cannot be resolved safely.
- Never infer ownership from `homebrew_version` text such as `(mise)`.

Do not compare versions with semver ordering. Version strings are opaque.

Allowed transitions:

| From                             | Event                                          | To                                                             |
| -------------------------------- | ---------------------------------------------- | -------------------------------------------------------------- |
| `Absent`                         | successful mise payload pour                   | `MiseOnly`                                                     |
| `PayloadOrphan`                  | implicit action                                | unchanged; require explicit recovery                           |
| `MiseOnly`                       | exact intent recorded                          | `MiseInteropPending`                                           |
| `MiseInteropPending`             | complete matching tree published               | `MiseInterop`                                                  |
| `MiseInteropPending`             | explicit rollback                              | `MiseOnly`                                                     |
| `MiseInterop`                    | mise exact upgrade                             | upgrade-pending with old+new manifests, then new `MiseInterop` |
| `MiseInterop`                    | valid Homebrew rewrite with stale mise receipt | `Externalized`                                                 |
| `MiseInterop`                    | Homebrew removes mise payload ledger           | `HomebrewOwned` or `Absent`                                    |
| `HomebrewOwned` / `Externalized` | implicit mise action                           | unchanged; reject mutation                                     |
| any state                        | unexpected partial/mismatched bytes            | `Conflict`                                                     |

No automatic transition leaves `HomebrewOwned`, `Externalized`,
`PayloadOrphan`, or `Conflict` for a mise-owned state. A valid external rewrite
may transfer authority away from mise; malformed or unexplained bytes never do.

**Verify**: `rtk cargo test cask_ownership` -> table-driven tests cover all eight
states, fresh/upgrade pending phases, valid external migration, malformed JSON,
partial state, orphan payload, and different-version metadata.

### Step 3: Block implicit ownership transfer before payload mutation

Call classifier in `install_one` before archive extraction, app/pkg changes,
stale cleanup, or metadata writes.

- `HomebrewOwned` or `Externalized`, normal apply: count valid payload as
  installed and preserve it. Explicit mise upgrade fails before mutation with
  exact recovery choices from Plan 008.
- `PayloadOrphan` or `Conflict`: fail every mutating path and point to Plan 008
  recovery. Do not silently adopt.
- `MiseOnly`: continue mise install/upgrade, but do not emit Homebrew metadata in
  this plan.
- `MiseInteropPending`: preserve unexplained paths and defer phase-aware
  recovery to Plans 007 and 003; do not start another install transaction.
- `MiseInterop`: permit only matching-provenance metadata mutation. Plan 002
  will supply the writer; until then, preserve existing bytes.

Remove unconditional fresh-pour writer call and automatic historical backfill
from current branch. Keep helper code only if Plan 002 will reuse it and tests
prove it cannot be reached without an ownership decision.

**Verify**: `rtk cargo test cask_ownership` -> an outdated `HomebrewOwned` fixture
returns error before any payload or metadata sentinel changes.

### Step 4: Add byte-preservation regression tests

For `HomebrewOwned` and conflict fixtures, seed:

- tab bytes;
- `config.json` bytes;
- two versioned installed caskfiles;
- a payload sentinel.

Run the ownership decision path. Assert every byte and path remains unchanged.
Also assert a plain mise receipt cannot authorize metadata deletion.

**Verify**: `rtk cargo test preserves_foreign_homebrew_metadata` -> all tests pass.

## Test plan

- Old `CaskReceipt` parses with no provenance.
- Matching provenance yields `MiseInterop`.
- Same-version Homebrew caskfile plus mise receipt but no provenance yields
  `Conflict`, not mise ownership.
- Different-version Homebrew metadata is detected; current-version-only lookup
  must not miss it.
- One valid Homebrew marker plus a stale old mise version/receipt is
  `Externalized`; multiple competing installed markers are `Conflict`. Neither
  may fall through to `Missing` or reinstall-over.
- Invalid tab/config/caskfile JSON yields `Conflict` and no mutation.
- Outdated Homebrew-owned cask cannot be upgraded by mise implicitly.
- No test may call real Homebrew or modify `/opt/homebrew`.

## Done criteria

- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] Ownership table tests cover all eight states, pending phases, and
      malformed metadata.
- [ ] No unconditional call to `write_homebrew_cask_metadata` remains.
- [ ] Foreign metadata fixture remains byte-identical after attempted upgrade.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.
- [ ] Only in-scope files changed, plus `plans/README.md` status.

## STOP conditions

Stop and report if:

- ownership cannot be classified without reading or changing Homebrew-owned
  payload outside Caskroom;
- preserving Homebrew-owned metadata requires changing formula behavior;
- a safe path appears to require deleting legacy synthetic metadata;
- version comparison appears necessary; redesign around exact equality;
- in-scope files drifted from current-state excerpts.

## Maintenance notes

- Review every future metadata mutation against `CaskOwnership`; helper-level
  checks alone are insufficient if callers bypass them.
- Provenance is mise's authorization record, not proof Homebrew accepts format.
- Legacy synthetic metadata remains a conflict until user explicitly repairs or
  reinstalls. Never guess its owner.
