# Plan 010: Retire unsafe synthetic Homebrew cask metadata

> **Executor instructions**: Follow every step and verification gate. This is
> an immediate safety correction, not implementation of a replacement interop
> format. Do not preserve the current writer behind a flag. Update this plan's
> status in `plans/README.md` when done.
>
> **Drift check (run first)**:
> `rtk git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs docs/bootstrap/packages/brew.md docs/dev/brew-cask-homebrew-interop.md`
> If the synthetic writer or its callers changed, re-audit the live code before
> editing. Mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug / security / docs
- **Planned at**: commit `866916893`, 2026-07-23
- **DONE**: 2026-07-23 — removed `write_homebrew_cask_metadata` / repair paths;
  regression tests prove no `.metadata` on mise pour + foreign metadata preserved;
  docs state mise-only ownership. Evidence: focused cask suite 70 passed.

## Why this matters

The branch writes Homebrew's private installed marker with an empty artifact
tab. Homebrew treats that marker as lifecycle authority, but an empty tab can
make its loader recover uninstall behavior from the current API. Mise therefore
claims authority it cannot describe and may let a later Homebrew operation
mutate the wrong historical files. The safe baseline is mise-owned payload and
mise receipt only.

## Current state

- `src/system/packages/brew/cask.rs:103-190` repairs or publishes synthetic
  metadata during normal install.
- `write_homebrew_cask_metadata` removes prior metadata version directories,
  writes `INSTALL_RECEIPT.json`, then publishes an empty installed JSON marker.
- `homebrew_cask_install_receipt` deliberately writes
  `"uninstall_artifacts": []`.
- `docs/bootstrap/packages/brew.md` and
  `docs/dev/brew-cask-homebrew-interop.md` describe Homebrew visibility as a
  supported result of the Rust pour.
- `.mise-cask.toml` is mise's own payload receipt and remains required.

## Commands you will need

| Purpose       | Command                                                             | Expected on success                                                                  |
| ------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Focused tests | `rtk cargo test system::packages::brew::cask`                       | all tests pass; no zero-test filter                                                  |
| E2E           | `rtk mise run test:e2e e2e/cli/test_system_install_brew_macos_slow` | pass on the supported macOS environment, or documented environment skip before merge |
| Docs          | `rtk mise run docs:build`                                           | exit 0                                                                               |
| Diff          | `rtk git diff --check`                                              | no output                                                                            |

## Scope

**In scope**:

- `src/system/packages/brew/cask.rs`
- its unit tests in the same file
- `docs/bootstrap/packages/brew.md`
- `docs/dev/brew-cask-homebrew-interop.md`
- `plans/README.md` status only

**Out of scope**:

- writing a different private Homebrew receipt;
- invoking Homebrew for install/adoption;
- deleting metadata from existing user machines;
- reconstructing old state from current API;
- dependency or ownership model redesign.

## Git workflow

- Branch: `fix/retire-brew-cask-metadata`
- Commit: `fix(brew-cask): stop publishing unsafe metadata`
- Use `git commit -s` and include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Make mise-only ownership the code baseline

Remove both `write_homebrew_cask_metadata` call paths from `install_one`:
normal post-install publication and already-installed repair. Preserve normal
installation, `.mise-cask.toml`, status, and dry-run behavior.

**Verify**:
`rtk rg -n "write_homebrew_cask_metadata|homebrew_cask_metadata_needs_repair" src/system/packages/brew/cask.rs`
→ no production call sites.

### Step 2: Delete the unsafe private-format implementation

Delete the writer, repair probe, empty Homebrew tab generator, timestamp helper,
and tests whose only contract is the synthetic layout. Preserve helpers used by
mise receipts or payload detection. Do not replace the empty tab with API JSON;
that describes a cask definition, not completed actions.

**Verify**:
`rtk rg -n "INSTALL_RECEIPT.json|uninstall_artifacts|\.metadata" src/system/packages/brew/cask.rs`
→ no Homebrew receipt publication remains.

### Step 3: Correct user and developer documentation

State that the Rust cask path is mise-owned, Homebrew commands must not manage
it, and an explicit supported handoff is under evaluation. Remove claims that
`brew list`, `brew upgrade`, `brew reinstall`, or `brew uninstall` work on a
mise-owned cask. Do not promise `--adopt` until Plan 012 passes its gates.

**Verify**:
`rtk rg -n "brew (list|upgrade|reinstall|uninstall).*mise|Homebrew-compatible" docs/bootstrap/packages/brew.md docs/dev/brew-cask-homebrew-interop.md`
→ no positive lifecycle promise for mise-owned casks.

### Step 4: Add regression assertions

Add/adjust tests proving a successful mise cask install writes
`.mise-cask.toml` but does not create `.metadata`. Add a test proving a healthy
existing mise receipt does not trigger Homebrew metadata repair.

**Verify**:
`rtk cargo test system::packages::brew::cask`
→ all focused tests pass and the new assertions execute.

## Test plan

- Successful mise-only cask install: payload receipt exists, `.metadata` absent.
- Repeated apply: no Homebrew marker appears.
- Existing foreign `.metadata`: preserved byte-for-byte; mise does not rewrite
  or delete it.
- Documentation build and focused Rust suite.

## Done criteria

- [ ] No production code creates or repairs Homebrew cask metadata.
- [ ] Existing foreign Homebrew metadata is never deleted or rewritten.
- [ ] Mise receipt/status behavior remains tested.
- [ ] Docs describe truthful mise-only ownership.
- [ ] Focused tests and docs build pass.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if removing the writer breaks mise's own installed detection;
if any cleanup would delete existing user metadata; if a replacement requires
private Homebrew files; or if focused test filtering executes zero tests.

## Maintenance notes

This is the safe foundation for every later option. A future handoff may ask
Homebrew to create its own metadata, but mise must not resume direct private
receipt publication without an explicit new decision.
