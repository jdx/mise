# Plan 002: Emit exact transactional Homebrew metadata for eligible binary casks

**2026-07-23 supersession note**: This is no longer a production-direction
plan. Execute only after Plans 011-013 and an explicit operator decision to
maintain a default-off private-format experiment. Prefer a proven native
Homebrew-owned handoff. Never infer actions from `CaskArtifacts`; consume the
completed-action manifest from Plan 013.

> **Executor instructions**: Follow every step and verification gate. Stop on
> any STOP condition; do not widen eligibility to make tests pass. Update this
> plan's status in `plans/README.md` when complete.
>
> **Research update 2026-07-23 (binding)**: do **not** install verbatim
> pour-time API JSON. It is version-exact but not pour-exact: Codex includes
> generated completions mise skips, API platform `variations` may be
> unresolved, and mise stages renamed binaries under target-derived paths.
> Homebrew treats installed-JSON artifacts as authoritative, so the tab cannot
> remove those extra actions. Use current Homebrew format instead: minimal
> installed JSON (`{}` unless the actual staged layout needs
> `url_specs.only_path`) plus a non-empty exact tab projected from filesystem
> actions mise actually completed. Raw API `Value` is retained only to resolve
> variations, detect unsupported lifecycle fields, and build fixtures. Full
> API JSON is never installed authority.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs src/system/packages/brew/cask_metadata.rs src/system/packages/brew/cask_ownership.rs`
> Plan 001 is expected to change these files. Confirm its ownership enum and
> provenance schema exist before continuing.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/001-model-cask-ownership.md` and
  `plans/006-cask-dependencies.md`
- **Category**: bug / security / architecture
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Current branch writes `{}` plus `uninstall_artifacts: []`. Homebrew interprets
that as missing installed metadata and fetches the current API definition. If
artifact paths changed, Homebrew can remove paths mise never installed or miss
paths mise did install. Codex currently exposes this: its API contains generated
completion artifacts, while mise deliberately skips them.

Safe dual-ledger rule: Homebrew metadata may exist only when it exactly describes
the installed version and actual layout. First supported class is binary-only
casks because mise and Homebrew both stage the binary in Caskroom and symlink it
into a permitted prefix path. Other layouts remain mise-owned until Plan 004
proves parity.

## Current state

- `src/system/packages/brew/cask.rs:916-966` parses installable app, binary, pkg,
  and font artifacts; it treats completions, hooks, uninstall, and zap as
  non-install artifacts.
- `src/system/packages/brew/cask.rs:1553-1589` ignores `CaskArtifacts` and writes
  an empty lifecycle ledger:

  ```rust
  fn homebrew_cask_install_receipt(
      cask: &Cask,
      _artifacts: &CaskArtifacts,
  ) -> serde_json::Value {
      // ...
      "uninstall_flight_blocks": false,
      "uninstall_artifacts": [],
  }
  ```

- `src/system/packages/brew/cask.rs:1485-1488` writes `{}` as installed JSON.
- Homebrew current contract:
  - installed JSON is deliberately minimal and relies on
    `INSTALL_RECEIPT.json` for exact installed version and uninstall artifacts:
    <https://docs.brew.sh/rubydoc/file.json_api_postinstall_preflight_postflight_plan.html>.
  - empty artifact metadata triggers current API fallback:
    <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/cask_loader.rb#L462-L474>
    and
    <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/cask_loader.rb#L841-L873>.
  - Homebrew's own `Tab.create` stores
    `cask.artifacts_list(uninstall_only: true)`:
    <https://docs.brew.sh/rubydoc/Cask/Tab>.

Current official Codex API contains a binary, generated completions, and zap:
<https://formulae.brew.sh/api/cask/codex.json>. Mise currently installs only
the binary. Exact metadata must not claim mise installed those completions.

## Commands you will need

| Purpose          | Command                                       | Expected on success |
| ---------------- | --------------------------------------------- | ------------------- |
| Focused tests    | `rtk cargo test homebrew_cask_metadata`       | exit 0              |
| Full cask module | `rtk cargo test system::packages::brew::cask` | exit 0              |
| Lint             | `rtk mise run lint`                           | exit 0              |
| Diff check       | `rtk git diff --check`                        | no output           |

If `rtk cargo` cannot find Cargo on this workstation, use
`rtk proxy /Users/donbeave/.cargo/bin/cargo` with identical arguments.

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- `src/system/packages/brew/cask_metadata.rs` (create)
- `src/system/packages/brew/cask_ownership.rs` from Plan 001
- `settings.toml` and generated settings/schema/docs for an explicit interop
  opt-in
- focused unit fixtures under `test/fixtures/brew-cask-metadata/` if needed

**Out of scope**:

- App, font, pkg, completion, manpage, pre/postflight, uninstall-flight-block,
  or installer-script interoperability.
- Automatic backfill of legacy receipts lacking exact snapshots.
- Homebrew CLI invocation.
- Driver reconciliation and real-Homebrew E2E; Plan 003 owns those.
- Mise-managed upgrades of an existing interop cask; Plan 007 owns the
  cross-payload transaction.
- Claiming that all `brew-cask:` installs are Homebrew-visible.

## Git workflow

- Branch: `advisor/002-exact-binary-cask-metadata`
- Commit: `fix(brew-cask): snapshot exact brew metadata`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Isolate the private compatibility adapter

Create `src/system/packages/brew/cask_metadata.rs`. Move all Homebrew-specific
path, timestamp, tab, installed JSON, validation, and transaction logic there.
Keep artifact install logic in `cask.rs`.

Expose a narrow API resembling:

```rust
pub(super) struct MetadataSnapshot { /* versioned exact data */ }
pub(super) enum Eligibility { Eligible(MetadataSnapshot), Ineligible(Reason) }

pub(super) fn classify_eligibility(
    cask: &Cask,
    artifacts: &CaskArtifacts,
) -> Result<Eligibility>;

pub(super) fn stage_metadata(
    token_dir: &Path,
    snapshot: &MetadataSnapshot,
) -> Result<StagedMetadata>;

// Publishes only bytes already hashed in StagedMetadata.
pub(super) fn publish_metadata(staged: StagedMetadata) -> Result<()>;
```

Keep adapter types private to `brew`. Add a schema constant and pin source
compatibility comments to Homebrew commit `c010c96b`.

**Verify**: `rtk cargo test homebrew_cask_metadata_module` -> module compiles;
existing metadata tests moved without behavior loss.

### Step 2: Build an immutable snapshot from actual actions

Snapshot must include:

- opaque token and version strings;
- lossless raw API `serde_json::Value` captured alongside the typed `Cask`, then
  resolved for the active platform before classification; current
  `HTTP_FETCH.json_cached::<Cask>` discards unknown fields and is insufficient;
- effective tap derived from qualified request when API `tap` is absent;
- `tap_git_head` captured at pour time;
- exact canonical uninstall artifacts for files mise actually installed;
- exact declarative zap/uninstall metadata only when losslessly canonicalized
  from this version's fetched API/source;
- `uninstall_flight_blocks` truthfully set;
- config values matching actual target directories;
- selected languages and other loader config that affects the projected
  version's artifacts;
- current minimal installed JSON: `{}` unless actual staged source resolution
  requires `url_specs.only_path`; never include raw API `artifacts`;
- a canonical tab artifact list projected from actual staged sources/targets,
  not copied blindly from API stanzas. For renamed binaries, source must resolve
  to the file mise retained under Caskroom so Homebrew rollback can relink it;
- exact `runtime_dependencies` from Plan 006 for every installed dependency.

For v1 eligibility, allow only:

- one or more binary artifacts whose actual Caskroom source and target symlink
  both exist and match the parsed install plan;
- no app, font, or pkg artifacts;
- no preflight/postflight or uninstall flight blocks;
- no unresolved platform/language variation, `old_tokens`, tap migration,
  container override, or unknown lifecycle-affecting field;
- no `version: latest` or `sha256: no_check` until Plan 008 adds dedicated
  lifecycle fixtures;
- every formula/cask dependency installed with correct dependency provenance,
  exact recursive closure, and tab representation from Plan 006; an ineligible
  dependency makes the root ineligible;
- no skipped activatable artifact that already exists at its Homebrew target;
- only declarative uninstall/zap entries representable without Ruby execution.

Generated completions skipped by mise must be omitted from both installed JSON
and tab. Their targets must not exist. The real Homebrew upgrade E2E must prove
that a later Homebrew takeover installs successor completions without using
them to uninstall the predecessor. If canonical Homebrew shape cannot be
derived with confidence, return `Ineligible`; never install raw API JSON.

Store eligible snapshot data in `.mise-cask.toml` at pour time even when
interop publication is disabled. The setting controls marker publication, not
historical capture. This permits a later explicit apply to publish from the
immutable snapshot without live API. Old receipts without snapshot remain
ineligible for automatic repair.

**Verify**: `rtk cargo test exact_binary_snapshot` -> after Plan 006, Codex
fixture includes actual binary source/target and ripgrep dependency, omits
generated completions from both authorities, uses exact version/tap, and never
falls back to an empty artifact array or current API.

### Step 3: Add golden compatibility fixtures

Create small JSON fixtures generated from Homebrew `Cask::Tab.create` semantics
for:

- one default binary target;
- renamed binary target;
- multiple binaries;
- absolute permitted `/usr/local` target;
- Codex-like binary plus skipped completion plus declarative zap;
- third-party tap.

Fixtures must contain no machine-specific paths or user data. Record Homebrew
commit and generation command in a fixture README. Tests compare canonicalized
JSON structures, not pretty-print whitespace.

Add negative fixtures for app, pkg, font, Ruby hooks, malformed tap, and an
unknown artifact. Every negative fixture must return a specific ineligibility
reason and produce no `.metadata` tree.

**Verify**: `rtk cargo test homebrew_metadata_fixture` -> all positive and
negative cases pass.

### Step 4: Publish fresh-install metadata transactionally

For a fresh eligible `MiseOnly` install only, use a recoverable two-ledger
protocol. No filesystem primitive can atomically replace both
`.mise-cask.toml` and Homebrew's `.metadata`, so ordering is part of the safety
contract. Existing `MiseInterop` upgrade belongs to Plan 007.

1. Acquire per-token `crate::lock_file::LockFile`.
2. Re-read ownership and fingerprints after lock acquisition.
3. Build the complete replacement under a transaction directory inside the
   installed version directory. Never create mise-private files/directories at
   `Caskroom/<token>/` root or `.metadata/` root; Homebrew expects to remove
   those roots and diagnoses leftovers as corrupt.
4. Parse and validate every generated JSON file.
5. Write installed caskfile last inside the temp tree, then hash the exact final
   bytes and paths.
6. Atomically rewrite `.mise-cask.toml` with the immutable snapshot and intended
   provenance hashes. At this point ownership is `MiseInteropPending`; Homebrew
   still sees no new marker.
7. Rename the staged tree into place. The single rename exposes a complete exact
   marker and moves ownership to `MiseInterop` by observation—no second receipt
   write is needed.
8. On a synchronous publish failure, restore the `MiseOnly` receipt. On process
   death after Step 6, retain the exact fresh-pending record so Plan 003 can
   deterministically roll forward; never reconstruct from live API.
9. Remove staging only after both ledgers validate. Cleanup may use only paths
   and hashes recorded in the transaction.

Never merge into or replace `HomebrewOwned`, `Externalized`, or `Conflict`. Never use the current
non-atomic `crate::file::write` directly on live metadata.

Use a temp/backup name containing a mise prefix and transaction ID. Cleanup may
remove only names proven self-authored. The timestamp directory must match
Homebrew's `%Y%m%d%H%M%S.%L` parser exactly.

**Verify**: `rtk cargo test homebrew_metadata_transaction` -> injected failure
or simulated process death at every numbered write/swap point yields exactly
`MiseOnly`, `MiseInteropPending`, or `MiseInterop`; no partial installed marker
or unclassified state survives.

### Step 5: Gate and wire eligibility into fresh install

Add `brew_cask_homebrew_interop` to `settings.toml`, default `false`, exposed as
`MISE_BREW_CASK_HOMEBREW_INTEROP`. Enabling it also requires
`experimental = true`; otherwise return the standard experimental-feature
error. Render generated settings/schema/docs. This is a safety kill switch for
an unversioned private Homebrew interface, not a temporary test-only flag.

After successful payload install:

- opt-in enabled + eligible + `MiseOnly`: commit exact metadata/provenance;
- opt-in disabled: remain `MiseOnly` with the pour-time exact snapshot; never
  emit `.metadata`;
- ineligible: keep mise-owned payload, write no Homebrew installed caskfile,
  and log one precise informational line explaining unsupported interop class;
- existing interop/foreign/externalized/conflict: Plan 001 behavior wins; do not
  mutate in this plan.

Metadata failure must roll back compatibility state. Decide explicitly whether
the payload install returns error or succeeds as `MiseOnly`; whichever policy is
chosen, add a test and make Plan 003 reconciliation consistent with it. Do not
leave a failed command whose next normal apply silently skips repair.

**Verify**: `rtk cargo test writes_homebrew_metadata_only_when_exact` -> binary
fixture gets metadata; every ineligible/foreign fixture gets none and preserves
sentinels.

## Test plan

- Exact projected binary snapshot for default, renamed, multiple, absolute-target, and
  third-party-tap cases.
- Codex fixture omits skipped generated completions.
- Verbatim/raw API artifact list is never written.
- Opt-in off leaves a valid `MiseOnly` install and no metadata.
- Snapshot contains non-empty authoritative artifact list; no current API
  fallback is required.
- App/pkg/font/hook fixtures fail closed.
- Foreign metadata byte preservation.
- Failure/process-death injection before/after each transaction boundary,
  including deterministic pending roll-forward.
- Concurrent two-mise-writer test serializes on per-token lock.
- Timestamp parse fixture and root-layout assertion: after publication no
  mise-private entry exists at token or metadata root.
- Version strings remain opaque and use equality only.

## Done criteria

- [ ] Homebrew adapter isolated in its own module.
- [ ] No branch-generated eligible receipt contains empty
      `uninstall_artifacts`.
- [ ] Installed JSON is current minimal shape; tab is exact actual-action
      authority; raw API JSON is never installed.
- [ ] Codex-like receipt describes only actual installed artifacts.
- [ ] Legacy receipt cannot trigger automatic synthesis from current API.
- [ ] Foreign/conflict metadata never changes.
- [ ] Transaction fault tests prove rollback/no partial marker.
- [ ] Cross-ledger ordering never exposes Homebrew authority before mise records
      the exact intended hashes.
- [ ] Interop remains experimental and default-off.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- exact Homebrew canonical artifact shape requires executing untrusted Ruby;
- a binary's actual staged source/target differs from snapshot inputs;
- app/pkg/font/hook behavior is needed to make Codex fixture eligible;
- Homebrew current source no longer matches pinned loader/tab assumptions;
- transaction requires mutating foreign metadata to succeed;
- Plan 001 ownership states are absent or ambiguous.

## Maintenance notes

- Private adapter needs scheduled drift checks. Homebrew changed installed JSON
  behavior materially in July 2026.
- Non-empty exact artifact snapshot is safety-critical. Empty is not a harmless
  default.
- Eligibility may shrink on unknown input; fail closed.
- Plan 004, not ad hoc edits here, expands artifact coverage.
