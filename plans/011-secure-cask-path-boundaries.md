# Plan 011: Enforce cask path boundaries before filesystem mutation

> **Executor instructions**: Treat cask API fields as untrusted path input.
> Fix the enabling condition centrally; do not scatter string checks around
> individual call sites. Run every verification and update `plans/README.md`.
>
> **Drift check (run first)**:
> `rtk git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs`
> If path construction moved or validation already exists, compare every path
> sink below against live code. Mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `866916893`, 2026-07-23
- **DONE**: 2026-07-23 — `SafePathComponent` / `CaskIds`, checked joins, component-aware
  `app_target_path`, `$APPDIR` contain-after-expand, **request-token equality**
  (`ensure_cask_token_matches_request` in `fetch_cask`; accepts `old_tokens`/`aliases`);
  existing-symlink rejection at mutation boundaries; adversarial unit tests. No semver ordering.

## Why this matters

The current code joins API-provided token and version strings into cache and
Caskroom paths and accepts app targets using lexical prefix checks. `..`,
absolute paths, or crafted target components can escape intended roots before
any ownership or transaction mechanism runs. Interop cannot be made safe while
its storage boundary is attacker-controlled.

## Current state

- `fetch_cask` accepts API `token` and opaque `version` without path validation.
- archive caching and extraction use those fields in filesystem paths.
- `caskroom_token_dir`, `caskroom_version_dir`, and stale-version cleanup join
  the raw fields under the Homebrew prefix.
- `app_target_path` replaces `$HOMEBREW_PREFIX`, then accepts absolute paths by
  lexical `starts_with`; a path containing `..` can still escape after OS
  resolution.
- Version strings are opaque. Validation must establish safe containment, not
  semver parsing or ordering.

## Commands you will need

| Purpose       | Command                                                                                                         | Expected on success                    |
| ------------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| Locate sinks  | `rtk rg -n -e "cask\.token" -e "cask\.version" -e "target_name\(" -e "join\(" src/system/packages/brew/cask.rs` | every untrusted path sink reviewed     |
| Focused tests | `rtk proxy /Users/donbeave/.cargo/bin/cargo test system::packages::brew::cask`                                  | all pass                               |
| Lint          | `rtk mise run lint-fix`                                                                                         | exit 0; stage resulting relevant fixes |
| Diff          | `rtk git diff --check`                                                                                          | no output                              |

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- unit tests in that file
- `plans/README.md` status only

**Out of scope**:

- semver validation or sorting;
- expanding allowed artifact target roots;
- following symlinks outside an allowed root;
- Homebrew metadata or handoff behavior;
- unrelated package managers.

## Git workflow

- Branch: `security/brew-cask-path-boundaries`
- Commit: `security(brew-cask): contain untrusted paths`
- Use `git commit -s` and include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Inventory fields by path policy

Build a test-backed table in code comments or tests:

- token and version: exactly one non-empty normal path component;
- archive/cache filename: derived from parsed URL filename, one component;
- app target: normalized absolute path under `/Applications` or
  `<prefix>/Applications`, with no `ParentDir`, root substitution, or prefix
  confusion;
- relative artifact source: normalized relative path, no root/prefix/parent;
- binary/font destination: contained in the existing explicit root allowlist.

Do not silently sanitize. Reject invalid values with the cask token/field name
and no filesystem mutation.

**Verify**:
`rtk rg -n "validate_.*component|checked_.*join|contained_.*path" src/system/packages/brew/cask.rs`
→ one central validation layer exists (names may differ).

### Step 2: Introduce validated path types/helpers

Create a small validated component type for token/version and a checked-join
helper that rejects `RootDir`, `Prefix`, `ParentDir`, and empty paths. Preserve
the original opaque version text after validation. For absolute target roots,
normalize components lexically before containment checks; never use raw string
prefixes. If existing targets may be symlinks, validate the parent/root using a
race-resistant open/canonicalization strategy before mutation or fail closed.

**Verify**: unit tests accept opaque safe versions such as `latest`,
`2026.07.23`, `1.2,3`, and `preview-1`, while rejecting traversal and absolute
components.

### Step 3: Validate once before any side effect

Perform token/version and artifact path validation immediately after parsing
the cask definition and before bootstrap, download, extraction, hooks, `sudo`,
directory creation, removal, or rename. Thread validated values through all
cache/Caskroom helpers so raw strings cannot reach path joins.

**Verify**:
`rtk proxy /Users/donbeave/.cargo/bin/cargo test system::packages::brew::cask`
→ invalid-input tests prove the temporary prefix/cache remains unchanged.

### Step 4: Replace lexical app containment

Refactor `app_target_path` to construct a normalized path from an enumerated
allowed root. Reject `..`, alternate root spellings, embedded NUL, and paths
that normalize outside the selected root. Keep the current explicit
`/Applications` and `<prefix>/Applications` policy only.

**Verify**: tests reject `/Applications/../tmp/Evil.app`,
`$HOMEBREW_PREFIX/Applications/../../bin/evil`, relative traversal, and a
prefix lookalike; valid app names and valid in-root absolute targets pass.

## Test plan

- Safe opaque token/version cases; no semver assumption.
- Token/version: empty, `.`, `..`, slash traversal, absolute path.
- Artifact source and URL filename traversal/encoded separator cases.
- App target allowed-root happy paths and traversal/prefix-lookalikes.
- Invalid input fails before hooks, package installer, download, or file writes.
- Existing binary/font root tests remain green.

## Done criteria

- [ ] Every API-derived path has an explicit policy and central validator.
- [ ] Validation runs before side effects.
- [ ] Token/version remain opaque after single-component validation.
- [ ] App containment is component-aware, not lexical string prefixing.
- [ ] Adversarial tests prove no out-of-root mutation.
- [ ] Focused tests and lint pass.

## STOP conditions

Stop and report if a legitimate current cask requires parent traversal; if a
supported artifact target must escape documented roots; if safety depends only
on `canonicalize` of a nonexistent final path; or if fixing a symlink race
requires a broader filesystem abstraction.

## Maintenance notes

All future cask artifact classes must enter through the same validation layer.
Never loosen component rules to accommodate one malformed upstream definition
without a documented, separately tested path policy.
