# Plan 006: Model cask constraints and dependency ownership exactly

**2026-07-23 supersession note**: Depend on Plan 013's completed-action truth.
A serialized dependency graph alone does not prove Homebrew autoremove safety;
add dependency drift, explicit-vs-dependency provenance, takeover, and
autoremove E2E. Treat dependency-bearing handoff as ineligible until those tests
pass.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. Stop
> on any STOP condition. Update this plan's row in `plans/README.md` when
> complete.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs src/system/packages/brew/mod.rs`
> Compare the excerpts under "Current state" with live code. Mismatch is a
> STOP condition. This plan is an eligibility prerequisite for Plan 002, not
> optional follow-up bookkeeping.

## Status

**CLOSED — NOT APPLICABLE after Plan 012.** This plan existed to make a
dependency-bearing cask eligible for Homebrew handoff/private metadata.
Handoff is unsupported and failed attempts empirically leave formula residue.
Mise does not serialize a Homebrew dependency ledger. General direct-pour
dependency support is a separate product feature, not an interop unblocker.

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/001-model-cask-ownership.md`; Plan 002 consumes this
  plan's exact dependency closure
- **Category**: correctness / ownership / parity
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Real `brew install --cask codex` installs the declared `ripgrep` formula and
records the resolved formula closure in the cask tab. Mise currently ignores
`depends_on`, so Codex is not eligible for Homebrew visibility even if its
binary artifact is exact.

This is lifecycle state, not display-only bookkeeping. Homebrew records the
resolved closure in the cask tab and reads it for installed missing-dependency
diagnostics. Current `uses`/installed-dependent/autoremove paths instead reload
the cask's declared `depends_on` and formula receipt closures. Both views must
agree. Formula receipts also distinguish an explicit request from a dependency
install; calling the existing formula path with its default request context
would incorrectly pin a new dependency as `installed_on_request=true`.

## Current state

- `src/system/packages/brew/cask.rs:1655` — `depends_on` is in the ignored
  keys list; nothing else in the cask manager touches dependencies.
- `src/system/packages/brew/mod.rs:41,57` — `BrewManager` (formula pour)
  lives in the same module and exposes formula installation
  (`install_via_pour`); the cask manager can delegate to it in-process.
- `Cask` struct (`cask.rs:32-51`) does not deserialize `depends_on`,
  `conflicts_with`, or platform variations as lifecycle constraints. The API
  object can contain formula/cask dependencies plus macOS, architecture, and
  other requirement shapes.
- Formula pours already write keg receipts (`pour.rs`), including
  `installed_on_request`; the cask orchestrator must pass dependency context
  without downgrading a formula that was already explicitly requested.
- Current Homebrew `Cask::Tab.runtime_deps_hash` records the resolved recursive
  cask and formula dependency graph and marks `declared_directly`; do not
  approximate it from only the raw direct list.

## Commands you will need

| Purpose         | Command                                       | Expected on success |
| --------------- | --------------------------------------------- | ------------------- |
| Cask unit tests | `rtk cargo test system::packages::brew::cask` | exit 0              |
| Formula tests   | `rtk cargo test system::packages::brew`       | exit 0              |
| Lint            | `rtk mise run lint`                           | exit 0              |
| Diff check      | `rtk git diff --check`                        | no output           |

If `rtk cargo` cannot find Cargo on this workstation, use
`rtk proxy /Users/donbeave/.cargo/bin/cargo` with identical arguments.

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs` (parse constraints; resolve dependency
  graph; orchestrate dependency installs)
- `src/system/packages/brew/mod.rs` and formula pour request context
- provenance/transaction modules introduced by Plan 001
- unit tests; macOS E2E extension for a dependency-bearing cask

**Out of scope**:

- Implementing an autoremove command. Correct installed-dependent and
  autoremove inputs are in scope.
- Guessing version constraints or requirement shapes.
- `depends_on` on formulae (formula deps already resolve via bottle metadata).
- Promoting a dependency-bearing cask to interop before the exact closure can
  be installed and serialized.

## Git workflow

- Branch: `advisor/006-cask-dependencies`
- Commit: `feat(brew-cask): install cask depends_on dependencies`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Parse the full eligibility surface

Retain the raw cask JSON beside the typed `Cask` and add typed models for known
`depends_on` formula/cask entries, macOS/architecture requirements, and
`conflicts_with`. Apply platform variation before interpreting these fields.
Treat all version strings as opaque.

Unknown dependency, requirement, conflict, or variation shapes may remain
parseable for mise-only installs, but make Homebrew interop ineligible with a
named reason. Never silently ignore an unknown lifecycle key.

**Verify**: `rtk cargo test cask_depends_on_parse` -> Codex-like fixture yields
the resolved platform formula list; unknown constraint fixture remains
mise-parseable but interop-ineligible.

### Step 2: Evaluate constraints before any mutation

Before downloading dependencies or payload, evaluate the resolved macOS,
architecture, and conflict constraints against actual state. Use Homebrew's
documented API encoding and macOS-version ordering only; never route arbitrary
tool versions through semver. Unmet requirements and conflicts fail with the
token and exact constraint. Unsupported shapes fail closed before mutation.

**Verify**: unit test with an unmet arch requirement produces an actionable
error and no filesystem mutation.

### Step 3: Resolve one exact dependency graph

Build a cycle-checked graph before fetching the root cask archive:

1. Resolve formula dependencies and their actual bottle dependency closure
   through the backend, not by sorting or inventing versions.
2. Resolve cask dependencies recursively with visiting/visited sets. A cycle
   is an error; do not use a depth limit as a substitute for cycle detection.
3. Deduplicate by canonical full name while preserving `declared_directly`.
4. Freeze exact resolved token/version/revision/full-name data into the root
   transaction journal before mutation.
5. `--dry-run` prints the topological order and whether each node already
   exists, will be installed, or blocks interop.

Record the exact direct and recursive dependency manifest in
`.mise-cask.toml` (additive, `serde(default)`) so status, repair, and metadata
generation use the same immutable result. A later live API response must not
rewrite an installed closure.

**Verify**: graph fixtures cover formula+cask recursion, aliases/full names,
diamond deduplication, cycles, exact versions, and platform variations.

### Step 4: Install dependencies with correct ownership provenance

Extend the internal formula install request with explicit request origin:
`UserRequested` or `CaskDependency { root_token }`.

- New formula dependency receipts use `installed_on_request=false` and
  `installed_as_dependency=true`.
- A formula already marked installed-on-request remains so; dependency use
  must never downgrade it.
- If the same formula is explicitly requested elsewhere in the current apply,
  `UserRequested` wins regardless of traversal/order; do not let graph timing
  change its receipt provenance.
- A formula already installed as dependency remains a dependency.
- Cask dependencies are installed in topological order and must themselves
  pass the same ownership/eligibility rules. If their lifecycle cannot be
  represented exactly, keep the root cask mise-only; never publish a partial
  Homebrew graph.
- Any failure before root payload mutation rolls back only new dependency
  state proven to be created by this transaction. Preserve pre-existing
  formulae/casks and their receipts byte-for-byte.

**Verify**: receipt tests prove new dependency, pre-existing explicit formula,
pre-existing dependency formula, same-apply explicit+dependency precedence,
rollback, and dry-run semantics.

### Step 5: Record exact runtime_dependencies in the cask tab

Expose the immutable manifest to Plan 002. Populate tab
`runtime_dependencies` in the current Homebrew shape, separated into `cask`
and `formula` collections. Include the resolved recursive graph exactly as
Homebrew's `Cask::Tab.runtime_deps_hash` does; set `declared_directly` from the
root declaration. Formula entries include actual `full_name`, `version`,
`revision`, and `pkg_version`. Read installed receipts/kegs; never guess.

If any dependency receipt is missing, ambiguous, or differs from the frozen
manifest, the root is not eligible for metadata publication. This is a
precondition to Plan 002's final marker rename.

**Verify**: semantic fixture comparison against a real Homebrew Codex tab and
CLI checks for `brew missing`, `brew deps --cask --installed`,
`brew uses --installed`, and autoremove protection. The latter two validate
the loaded cask declaration plus formula receipts, not the cask tab alone.
Compare exact resolved versions from the test graph, not whatever versions
happen to be current online.

## Test plan

- Parse fixtures: formula-only, cask+formula, macOS/arch requirements,
  conflicts, variations, unknown keys, absent constraints.
- Graph and orchestration order: dependencies before root; failure aborts root
  pre-mutation; rollback preserves pre-existing state.
- Cycle, alias, full-name, and duplicate handling.
- Formula provenance matrix for explicit versus dependency installs.
- Dry-run output lists dependencies.
- Dedicated disposable macOS E2E: a local fixture cask with formula and cask
  dependencies installs the exact graph; Homebrew CLI sees installed
  dependents; an autoremove dry run does not propose a needed dependency.

## Done criteria

- [ ] Codex-class cask pour installs its exact dependency graph.
- [ ] Unmet macos/arch requirements fail loudly pre-mutation.
- [ ] Conflict/unknown constraint shapes block interop before mutation.
- [ ] New formula dependencies are not marked installed on request; existing
      explicit formulae retain their provenance.
- [ ] Dependency manifests are recorded in `.mise-cask.toml`.
- [ ] Plan 002 tab `runtime_dependencies` matches Homebrew semantics.
- [ ] Installed-dependent/autoremove behavior passes on a disposable runner.
- [ ] `rtk cargo test system::packages::brew::cask` exits 0.
- [ ] `rtk mise run lint` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- the `depends_on` API shape does not match the parsed model (record the
  actual JSON);
- macos/arch requirement evaluation would require semver-style guessing not
  encoded in the API shape;
- recursive cask dependencies cannot be represented exactly (root stays
  mise-only; do not publish partial interop metadata);
- formula delegation would shell out to `brew` (never do this);
- formula install cannot preserve `installed_on_request` provenance;
- a dependency graph changes between resolution and publication;
- in-scope files drifted from current-state excerpts.

## Maintenance notes

- Dependency install remains in-process Rust pour; shelling to `brew` is an
  explicit non-goal (#10582).
- This plan does not run autoremove, but it must produce correct ownership
  inputs so Homebrew does not prune a still-needed dependency.
- Keep version strings opaque end to end.
