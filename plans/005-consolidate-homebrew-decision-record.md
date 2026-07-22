# Plan 005: Replace contradictory Homebrew findings with one decision record

**2026-07-23 supersession note**: Correct unsafe user-facing claims immediately
in Plan 010; do not wait for the full research program. Use this plan later to
collapse historical alternatives after Plan 012's handoff gate and any narrowed
Plan 009 outcome are decided.

> **Executor instructions**: Execute after Plans 001, 006, 002, 007, 008, 003,
> and Plan 009's local decision. Include only Plan 004 class gates completed at
> that time; unfinished gates remain explicitly future. Documentation must
> describe verified behavior, not intent. Preserve
> historical detail in git; do not keep mutually contradictory append-only conclusions in active
> documentation. Update `plans/README.md` when complete.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- HOMEBREW_FINDINGS.md docs/dev/brew-cask-homebrew-interop.md docs/bootstrap/packages/brew.md`
> Re-read all three changed files before editing. New claims not supported by
> implementation or tests are a STOP condition.

## Status

- **DONE (partial final)**: 2026-07-23 — `docs/dev/brew-cask-decision-record.md`
  ADR + support matrix; interop doc banner archives contradictions; Plan 012
  STOP recorded honestly. Revisit when disposable handoff evidence exists.

## Status (original)

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW
- **Depends on**: `plans/003-reconcile-and-test-cask-interop.md` and
  `plans/009-upstream-homebrew-registration-contract.md`; inspect completed
  Plan 004 gates
- **Category**: docs / maintainability
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

`HOMEBREW_FINDINGS.md` contains useful evidence, but it also contains opposing
recommendations: one section says blanket synthetic metadata must not ship,
while later sections call the same direction the winner. The dev interop page
duplicates much of it, and user docs overstate repair and lifecycle support.
Long append-only research is not a reliable current contract.

The repository needs one short architecture decision record (ADR), a truthful
user support matrix, and executable tests as the authority.

## Current state

Contradictions to resolve:

- `HOMEBREW_FINDINGS.md:34` says never destroy Homebrew-owned metadata; current
  writer deletes top-level version directories and overwrites the shared tab.
- Multiple appended passes reject blanket Direction A, later recommend it,
  then recommend verbatim API JSON. Plan 002 rejects both authority models.
- `HOMEBREW_FINDINGS.md:243-245` correctly records current-API recovery from
  empty metadata; later recommendations accept `{}` and empty artifacts anyway.
- `HOMEBREW_FINDINGS.md:647-649` requires disabling Codex's updater under mise
  ownership; later text makes it optional.
- `HOMEBREW_FINDINGS.md:670-697` requires exact lifecycle parity,
  transactions, and a full test matrix; `:705` still recommends keeping the
  incomplete receipt.
- `docs/bootstrap/packages/brew.md:127-133` says re-invocation repairs metadata,
  but normal apply filters installed packages before `install_one`.
- `HOMEBREW_FINDINGS.md:13` and later imperative rules are agent-operating
  instructions mixed into research. Stable agent policy belongs in
  `AGENTS.md`; research should describe evidence and decisions.

## Commands you will need

| Purpose             | Command                   | Expected on success               |
| ------------------- | ------------------------- | --------------------------------- |
| Render docs         | `rtk mise run render`     | exit 0; generated docs consistent |
| Build docs          | `rtk mise run docs:build` | exit 0                            |
| Search stale claims | commands in Step 4        | no unsupported claim remains      |
| Diff check          | `rtk git diff --check`    | no output                         |

## Scope

**In scope**:

- `HOMEBREW_FINDINGS.md`
- `docs/dev/brew-cask-homebrew-interop.md`
- `docs/bootstrap/packages/brew.md`
- one ADR under `docs/dev/` if that matches current repository convention
- related generated documentation from `mise run render`

**Out of scope**:

- Behavior changes.
- Repeating internal implementation details in user docs.
- Promising future artifact classes from Plan 004 as shipped.
- Moving stable repository policy out of `AGENTS.md`.

## Git workflow

- Branch: `advisor/005-cask-interop-docs`
- Commit: `docs(brew-cask): record safe interop contract`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless operator asks.

## Steps

### Step 1: Write a concise ADR with one current decision

Create or rewrite the dev interop document using this structure:

1. **Status/date/decision**: exact current support boundary and whether the
   private adapter remains experimental or an upstream contract exists.
2. **Goals**: Homebrew discovery plus safe lifecycle only where exact parity is
   proven; mise remains usable without Homebrew.
3. **Non-goals**: blanket markers, implicit adoption, live-API historical
   reconstruction, and universal artifact support.
4. **Ownership state machine**: all eight Plan 001 states, including pending
   phases, orphan payload, valid Homebrew takeover (`Externalized`), and
   conflict, with mutation authority.
5. **Safety invariants**: exact immutable installed snapshot, provenance,
   transaction, apply-time reconciliation, fail-closed eligibility.
6. **Current support matrix**: generated from implemented/tested behavior.
7. **Alternatives rejected** with concrete failure mode.
8. **Evidence**: pinned Homebrew sources and corresponding tests.
9. **Future gates**: name remaining unsupported classes; label each unshipped.

Use normative `MUST` only for an invariant enforced by code/test. Keep source
links pinned to a commit for behavior and link public docs for concepts.

**Verify**: reviewer can answer who may mutate each ledger, which casks are
Homebrew-visible, and what happens on missing metadata from this document alone.

### Step 2: Migrate backlinks, then retire the root findings dump

First migrate every active reference in `plans/README.md`, unfinished plans,
docs, and source comments to the ADR or a pinned upstream source. Verify no
current plan requires the findings dump to understand a safety invariant.

Then delete `HOMEBREW_FINDINGS.md` after migrating unique evidence to the ADR,
because git preserves research history. If an external or active PR link needs
a stable root target, reduce it to:

- archival status;
- one-sentence superseded verdict;
- link to the ADR and plan index;
- commit/date of the original investigation.

Do not retain repeated prompts, append-only discussion, temporary local-machine
observations, or contradictory recommendations in active docs. Never delete
first and leave the plan index or executor instructions pointing at a missing
authority.

**Verify**: repository search finds only one normative interop decision record.

### Step 3: Correct user-facing documentation

Update `docs/bootstrap/packages/brew.md` to state only shipped behavior:

- which cask artifact classes are Homebrew-visible;
- unsupported classes remain mise-managed and may not appear in `brew list`;
- normal apply reconciles only exact self-authored metadata;
- legacy/foreign/conflicting metadata is never synthesized or overwritten;
- Homebrew absence does not break mise payload install/status;
- tools that self-run `brew upgrade --cask` must disable that updater while
  their cask is mise-only/ineligible;
- a successful Homebrew upgrade of an eligible interop cask transfers lifecycle
  ownership to Homebrew; subsequent mise apply preserves that ledger;
- provide safe recovery instructions that do not tell users to delete foreign
  metadata.

Do not call metadata “Homebrew-compatible” without naming the tested lifecycle
operations and eligibility boundary.

**Verify**: every user-visible claim maps to a unit or macOS E2E assertion from
the completed prerequisite plans.

### Step 4: Add documentation drift checks

Run targeted searches and review every result:

```text
rtk rg -n "all casks|every cask|re-?invoke|empty.*artifact|Direction A|verbatim.*API|Homebrew-compatible" docs plans
rtk rg -n "HOMEBREW_FINDINGS\.md" . --glob '!target/**' --glob '!.git/**'
rtk rg -n "write_homebrew_cask_metadata|reconcile_installed|Eligibility" src docs
```

Add a short source comment/test name referenced by the ADR support matrix so a
behavior change forces deliberate documentation review. Do not create a brittle
test that merely matches prose.

**Verify**: no search result makes a broader claim than eligibility code and
real-Homebrew E2E prove.

### Step 5: Render and review links

Run documentation render/build. Inspect generated diffs, internal links, and
the final site path. Mise URLs must match directory structure, for example
`mise.en.dev/dev-tools/backends/...`, never shortened aliases.

**Verify**: `rtk mise run render`, `rtk mise run docs:build`, and
`rtk git diff --check` all exit 0.

## Test plan

- ADR support matrix matches adapter eligibility test cases.
- User docs distinguish exact self-authored repair from legacy/foreign state.
- No claim that install-only success proves upgrade/uninstall safety.
- No active document recommends empty artifact metadata or live-API fallback.
- No agent prompt/policy is embedded in research prose.
- All local and external links resolve during docs build.

## Done criteria

- [ ] One normative interop ADR remains.
- [ ] Contradictory root findings are deleted or clearly archived.
- [ ] No active backlink points to deleted findings content.
- [ ] User docs expose the exact shipped support boundary.
- [ ] Every lifecycle claim maps to executable evidence.
- [ ] Remaining unsupported work is labeled unshipped and links its current
      plan/issue rather than the retired findings dump.
- [ ] `rtk mise run render` exits 0.
- [ ] `rtk mise run docs:build` exits 0.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- prerequisite plan behavior is not merged or cannot be verified;
- implementation and tests disagree on an artifact's eligibility;
- deleting the root file would break an active external link that cannot be
  redirected;
- docs build generates unrelated changes that cannot be explained;
- a desired claim describes Plan 004 future work rather than shipped behavior.

## Maintenance notes

- Use git history for investigation chronology. Active docs state current
  contract.
- Update the support matrix in the same PR that promotes or demotes an artifact
  class.
- Private Homebrew internals can drift; pinned source plus behavior tests are
  stronger than prose copied from one version.
