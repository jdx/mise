# Homebrew cask interoperability remediation plans

## Verdict

Do not ship the current synthetic `.metadata` writer. Do not replace its empty
tab with full API JSON. Both grant Homebrew lifecycle authority that does not
match actions mise actually completed.

Use explicit single-owner modes:

1. **MiseOwned** — Rust pour, mise receipt, no Homebrew installed marker.
2. **HomebrewOwned** — Homebrew performs the installation and authors its own
   ledger from the start.
3. **Explicit one-way handoff** — classify and journal mise state, invoke a
   proven native Homebrew flow, verify it, then mark mise `Externalized` and
   refuse further mise mutation.

Homebrew now documents `brew install --cask --adopt`, which the earlier plan
missed. It is a full install/handoff, not receipt-only registration. Plan 012
must prove exact eligible classes, same-version Caskroom behavior, and rollback
in a disposable environment before production use.

The former A3 design—minimal installed JSON plus a non-empty tab projected from
completed actions—remains the least-wrong private-format experiment. It is
default-off research only. No shared lock is honored by both managers, so it is
not a durable product boundary.

## Execution order

| Order | Plan                                                                                    | Priority | Effort  | Risk | Status                                                                     | Depends on                                  |
| ----: | --------------------------------------------------------------------------------------- | -------- | ------- | ---- | -------------------------------------------------------------------------- | ------------------------------------------- |
|     1 | [Retire unsafe synthetic metadata](010-retire-unsafe-cask-metadata.md)                  | P1       | M       | MED  | **DONE** — writer/repair removed; foreign metadata preserved; suite green  | none                                        |
|     2 | [Enforce cask path boundaries](011-secure-cask-path-boundaries.md)                      | P1       | M       | HIGH | **DONE** — all untrusted path sinks contained; real pours pass             | none                                        |
|     3 | [Prove a supported Homebrew handoff](012-evaluate-homebrew-handoff.md)                  | P1       | L       | HIGH | **DONE** — unsupported; mise-only retained from disposable matrix          | 010                                         |
|     4 | [Record completed cask actions](013-record-completed-cask-actions.md)                   | P1       | L       | HIGH | **DONE** — durable journal, factual receipt, fingerprints, recovery health | 011                                         |
|     5 | [Model ownership and prevent takeover](001-model-cask-ownership.md)                     | P1       | L       | HIGH | **DONE** — takeover blocked; pending transactions unhealthy                | 011, 012, 013                               |
|     6 | [Model constraints and dependency ownership](006-cask-dependencies.md)                  | P1       | L       | HIGH | **CLOSED — NOT APPLICABLE** to handoff; direct-pour dependencies shipped   | 001, 013                                    |
|     7 | [Add safe diagnosis and recovery](008-cask-interop-recovery.md)                         | P1       | L       | HIGH | **CLOSED — NOT APPLICABLE**; no handoff/dual-ledger transition ships       | 001, 012, 013                               |
|     8 | [Reconcile and test real lifecycle](003-reconcile-and-test-cask-interop.md)             | P1       | L       | HIGH | **CLOSED — NOT APPLICABLE**; no Homebrew lifecycle claim                   | 008                                         |
|     9 | [Expand lifecycle parity by artifact class](004-expand-cask-lifecycle-parity.md)        | P2       | L/class | HIGH | **CLOSED — NOT APPLICABLE**; every handoff class excluded                  | 003                                         |
|    10 | [Narrow an upstream supported contract](009-upstream-homebrew-registration-contract.md) | P2       | M-L     | HIGH | **DONE** — local validation/atomic-handoff gap proposal; no contact        | 012                                         |
|    11 | [Emit exact binary private metadata](002-emit-exact-binary-metadata.md)                 | P3       | L       | HIGH | **CLOSED — NOT APPLICABLE**; private metadata direction rejected           | 001, 006, 013                               |
|    12 | [Transactional private interop upgrades](007-transactional-interop-upgrades.md)         | P3       | L       | HIGH | REJECTED — no shared manager lock                                          | supported contract or single-owner redesign |
|    13 | [Consolidate the decision record](005-consolidate-homebrew-decision-record.md)          | P2       | M       | LOW  | **DONE** — final unsupported support matrix and evidence                   | 010; 012 final outcome                      |
|    14 | [Prove representative direct-pour compatibility](014-direct-pour-compatibility.md)      | P1       | M       | HIGH | **IN PROGRESS** — local matrix green; disposable GitHub macOS gate pending | 011, 013                                    |

Plans 010 and 011 are unconditional safety work. Plans 012 and 013 establish
the supported handoff and truthful local state foundations. At the Plan 012
decision gate, choose Homebrew-owned native install, a proven class-limited
handoff, or mise-only mode. Do not execute Plans 001-009 unchanged: their
original execution order assumed A3 was the product direction.

## Corrected product-goal resolution

The original G1-G7 set treats Homebrew recognition as identity while excluding
the destructive lifecycle authorized by the same marker. Homebrew has no
identity-only cask marker.

| Goal                                        | Correct resolution                                                                        |
| ------------------------------------------- | ----------------------------------------------------------------------------------------- |
| G1: normal mise install needs no Homebrew   | preserved in `MiseOwned`; explicit handoff/native mode may invoke Homebrew by user choice |
| G2: feel like a Homebrew install            | only when Homebrew is the sole lifecycle owner                                            |
| G3: `brew list --cask --versions`           | guaranteed only after verified Homebrew-owned install/handoff                             |
| G4: `brew upgrade --cask`                   | guaranteed only after Homebrew assumes ownership and real lifecycle E2E passes            |
| G5: mise status/upgrade                     | status remains truthful; mise reports `Externalized` and blocks mutation after handoff    |
| G6: never destroy genuine Homebrew metadata | path safety, provenance, fail-closed classification, reversible recovery                  |
| G7: bootstrap-scoped                        | preserved; no general Homebrew replacement                                                |
| G8: one mutator at a time                   | explicit owner and phase; no dual-writer claim without a shared supported protocol        |
| G9: crash-safe transition                   | durable journal, exact completed actions, deterministic retry/rollback                    |

No correct local adapter can promise G2-G4 for every cask while deferring app,
pkg, hook, dependency, uninstall, zap, and rollback semantics.

## Deep-review findings

### 1. Installed metadata is lifecycle authority

Homebrew's installed caskfile and tab drive uninstall, reinstall, upgrade, zap,
dependency, and migration behavior. A marker is permission to mutate files.

Homebrew's loader uses non-empty tab artifacts as installed authority; an empty
list is treated as absent and can fall back to current API. Its current writer
deliberately emits minimal installed JSON because the tab carries installed
facts.

Evidence:

- [Installed JSON design](https://docs.brew.sh/rubydoc/file.json_api_postinstall_preflight_postflight_plan.html)
- [Loader and artifact fallback](https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/cask_loader.rb#L841-L873)
- [Uninstall path](https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/installer.rb#L957-L1021)
- [Private `Cask::Tab`](https://docs.brew.sh/rubydoc/Cask/Tab)

### 2. Verbatim API JSON is wrong authority

Raw pour-time JSON is useful input for platform resolution and unsupported-field
detection, but not an installed caskfile. It may authorize Homebrew to uninstall
generated completions, zap paths, app/pkg actions, or source paths that mise did
not create. The tab cannot subtract these actions because installed JSON
artifacts are also authoritative.

Correct Plan 002 authority is minimal installed JSON plus a non-empty tab
projected from verified filesystem actions. For renamed binaries, the recorded
source must be the exact retained Caskroom path Homebrew can relink during
rollback.

### 3. Ownership needs eight states, not “receipt exists”

A mise payload receipt proves payload history only. It does not authorize
overwriting Homebrew's ledger. Required states include absent, orphan payload,
mise-only, phase-aware pending, exact interop, Homebrew-owned, valid external
takeover, and conflict.

Normal Homebrew migration/upgrade can rewrite valid metadata and leave stale
mise provenance. That is `Externalized`, not corruption. Mise apply preserves
it; explicit mise upgrade fails before mutation.

### 4. Metadata-only transactions start too late

Current payload/link mutation completes before metadata writing. A crash can
leave new files under old Homebrew uninstall authority. A safe upgrade prepares
everything first, durably records old/new manifests, hides the old marker,
switches payload/targets with per-target journaling, and publishes new metadata
last as the visibility linearization point.

Atomic rename reduces exposure but does not create mutual exclusion with
Homebrew. No supported cross-manager lock exists.

### 5. Dependencies are an eligibility blocker

Codex declares the `ripgrep` formula dependency. Mise currently ignores cask
`depends_on`. Homebrew's cask tab records a recursive runtime dependency graph
and uses it for installed missing-dependency diagnostics. Current
installed-dependent and autoremove paths reload the cask's `depends_on` plus
formula receipt closures, so both views must agree. New formula dependencies
must be marked installed-as-dependency; an already explicit formula must retain
`installed_on_request=true`.

Partial dependency serialization makes the root cask ineligible. Plan 006 must
land before Codex-class interop.

### 6. Reconciliation and recovery are different

Normal apply currently filters installed packages before `install_one`, so the
existing repair path is unreachable. Add an explicit apply-time reconciliation
hook, but let it resume only a valid self-authored pending journal.

Missing metadata from a formerly converged install is `Conflict`, not pending.
Legacy receipts cannot be rebuilt from current API—even when version strings
match—because definitions can change without a version bump. Recovery needs a
read-only diagnostic plus explicit reversible quarantine/restore strategies.

### 7. Existing E2E can pass without testing Homebrew

The normal isolated harness does not reliably forward `CI`; a `CI=true` guard
can skip all Homebrew assertions. The current Hidden Bar pre-existence branch
can also exit the entire script. The harness isolates `HOME`, not the global
Homebrew prefix, `/Applications`, or pkg receipt database.

Required fix: explicit CI propagation, executed-scenario sentinel,
per-scenario skips, deterministic local tap/archive fixtures, and a dedicated
disposable macOS job. Pkg lifecycle tests require a disposable VM.

### 8. Artifact support is not lifecycle parity

Mise parsing/install support does not imply Homebrew-compatible layout. App
casks are the clearest counterexample: Homebrew's moved artifact leaves a
staged symlink and expects that layout during rollback; mise currently copies
and retains a real source directory. Binary-first eligibility is structural.

Every promoted class needs install, upgrade, uninstall, rollback, offline,
foreign-target, and mixed-class tests. Ruby uninstall flight blocks cannot use
JSON stubs. A later Plan 004 gate may preserve the exact checksum-bound trusted
`.rb` source Homebrew itself would install, but must independently prove trust,
single-caskfile selection, migration, execution, and rollback. Otherwise it
fails closed.

### 9. Native Homebrew handoff precedes a new upstream protocol

`Cask::Tab` and much of Caskroom are private. Local schema fixtures can detect
some future drift, but cannot protect markers already emitted. Homebrew's
supported `brew install --cask --adopt` path was missed in earlier passes.

Adoption still runs the normal Homebrew installer: fetch, stage, dependencies,
artifacts, installed caskfile, and tab. It is ownership transfer, not external
receipt registration. Plan 012 must test it before Plan 009 asks upstream for
anything. If gaps remain, Plan 009 should request the smallest missing supported
handoff feature—not begin with a broad arbitrary external-registration API.

### 10. Mise-private state location affects Homebrew cleanup

Homebrew recursively purges version directories, and extra mise files directly
under `Caskroom/<token>/` or `.metadata/` can block removal or trigger corruption
diagnostics. Durable transaction journals and reversible quarantine belong in a
same-volume prefix-owned recovery root such as
`<prefix>/var/mise/cask-recovery`, never under Homebrew-controlled token,
version, or metadata directories.

### 11. Path safety precedes ownership safety

API token/version and artifact target strings currently reach cache, Caskroom,
and activation paths without one central containment policy. Lexical prefix
checks do not neutralize `..` or symlink boundary problems. Plan 011 is an
unconditional prerequisite: validate opaque token/version components and every
artifact path before download, hooks, `sudo`, directory creation, removal, or
rename.

### 12. Installation intent is not completed action truth

The current `.mise-cask.toml` is projected from declarative `CaskArtifacts`
before binary/font activation completes. Ownership and recovery cannot safely
infer history from that intent or from the current API. Plan 013 makes mutators
emit a durable completed-action manifest and publishes the final receipt only
after required activation succeeds.

## Alternatives considered

| Alternative                                      | Verdict                  | Failure mode                                                                            |
| ------------------------------------------------ | ------------------------ | --------------------------------------------------------------------------------------- |
| Blanket `{}` + empty tab                         | rejected                 | current-API fallback drives historical teardown                                         |
| Verbatim pour-time API JSON                      | rejected                 | describes upstream definition, not mise's actual actions/layout                         |
| Identity-only marker                             | impossible               | Homebrew marker grants lifecycle authority                                              |
| Treat missing marker as package missing          | rejected                 | lies about installed payload and forces reinstall                                       |
| Backfill when current API version matches        | rejected                 | same-version definition edits remain possible                                           |
| Automatically adopt Homebrew installs            | rejected                 | no proof of payload or ledger ownership                                                 |
| Keep private adapter default-on with drift tests | rejected                 | cannot protect already-emitted metadata or coordinate locks                             |
| Blind `brew install --cask --adopt`              | rejected                 | full install; auto-update equality and same-version staging are unsafe until proven     |
| Proven native Homebrew handoff                   | preferred candidate      | Homebrew authors ledger and becomes sole owner; Plan 012 must pass                      |
| Homebrew install from the start                  | supported ownership mode | changes manager by explicit user choice; avoids dual authority                          |
| Broad upstream registration protocol first       | rejected                 | duplicates existing handoff surface and asks Homebrew to trust arbitrary external state |
| Narrow upstream handoff enhancement              | conditional              | pursue only for concrete gaps remaining after Plan 012                                  |

## Validation performed during planning

- Repository branch inspected at `866916893`; compared with origin main
  `89370a857` on 2026-07-23.
- Current Homebrew implementation and official documentation traced through
  cask loader, tab, installer, `--adopt`, upgrade, Caskroom, dependency,
  migration, and locking paths. Current upstream was re-verified at
  `33c3da5f49885a8e19170935f6e8515a66516cff`; relevant cask code is unchanged
  from the local Homebrew checkout.
- `rtk cargo test homebrew_cask`: 3 passed.
- `rtk cargo test system::packages::brew::cask`:
  63 passed.
- Three independent research agents audited local architecture, adversarial
  state/concurrency behavior, and current upstream Homebrew semantics. Their
  conclusions agreed on single-owner modes, immediate writer retirement, path
  safety, completed-action truth, and a native-handoff spike.

## Immediate operator safety

Until Plan 010 ships, do not create more synthetic `{}` + empty-tab metadata.
Do not run Homebrew upgrade/uninstall/reinstall against a token already carrying
it. Do not repair from current API or delete shared state by guess. Continue
managing the payload with mise only and preserve the full token tree for later
reversible diagnosis.

For a valid Homebrew-owned install, leave Homebrew metadata untouched. Do not
run `--adopt` experiments on the operator's real prefix. Returning ownership to
mise needs a separately proven withdrawal flow; never overlay one manager's
ledger on the other.

## Scope not audited

- Formula (`brew:`) interoperability except dependency provenance shared with
  casks.
- Linuxbrew; cask lifecycle is macOS-focused.
- Every historical Homebrew release and arbitrary third-party Ruby cask.
- Registry submissions, unrelated package managers, and implementation of the
  plans themselves.
