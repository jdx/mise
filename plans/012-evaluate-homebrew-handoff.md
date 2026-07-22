# Plan 012: Prove a supported Homebrew ownership handoff

> **Executor instructions**: This is a disposable-environment feasibility
> spike. Do not run install, adopt, force, uninstall, or zap experiments on the
> operator's normal Homebrew prefix or `/Applications`. Do not add production
> handoff code until the decision gate in Step 5 passes.
>
> **Drift check (run first)**:
> `rtk git diff --stat 866916893..HEAD -- src/system/packages/brew/cask.rs e2e/cli/test_system_install_brew_macos_slow docs/dev plans`
> Re-read current Homebrew install/adopt source and pin the tested commit.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: Plan 010 for the production safety baseline
- **Category**: direction / compatibility / tests
- **Planned at**: commit `866916893`, 2026-07-23
- **STOP (isolation unproven)**: 2026-07-23 — operator host has real
  `/opt/homebrew` + `/Applications`. No disposable mutation performed.
  Gate decision: **mise-only retained**; no production transfer. See
  `docs/dev/brew-cask-handoff-gate.md`.

## Why this matters

Homebrew already supports `brew install --cask --adopt`, but adoption is a full
Homebrew installation workflow, not receipt-only registration. It downloads
and stages the current Homebrew definition, handles dependencies and artifacts,
then Homebrew writes its own tab. This may provide the safest explicit one-way
handoff, but same-version Caskroom collisions, `auto_updates`, and non-app
artifacts must be falsified before product use.

## Current state

- Official docs describe `--adopt` for an already-present app:
  <https://docs.brew.sh/Tips-and-Tricks#adopt-a-manually-installed-app>.
- Current Homebrew source at `33c3da5f49885a8e19170935f6e8515a66516cff`
  runs normal `Cask::Installer#install`, including fetch, stage, dependencies,
  artifact phases, installed caskfile, and tab creation.
- `Moved` may accept differing content for `auto_updates`; adoption therefore
  does not always prove payload equivalence.
- Existing mise staging under `Caskroom/<token>/<version>` can collide with
  Homebrew's extraction/staging and may be purged after failure.
- Homebrew install/uninstall does not currently honor a lock mise can acquire.

## Commands you will need

| Purpose      | Command                                                                                                                                                        | Expected on success                                     |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| Pin upstream | `rtk git -C <homebrew-source> rev-parse HEAD`                                                                                                                  | commit recorded in results                              |
| Trace adopt  | `rtk rg -n -e adopt -e "def install" -e Tab.create -e save_caskfile <homebrew-source>/Library/Homebrew/cask <homebrew-source>/Library/Homebrew/cmd/install.rb` | every lifecycle phase mapped                            |
| E2E task     | `rtk mise run test:e2e e2e/cli/test_system_install_brew_adopt_macos_slow`                                                                                      | scenarios execute on disposable macOS; sentinel present |
| Diff         | `rtk git diff --check`                                                                                                                                         | no output                                               |

`<homebrew-source>` must be an explicit disposable checkout path, never an
unresolved environment variable in a destructive command.

## Scope

**In scope**:

- a deterministic local cask tap/archive fixture;
- a dedicated disposable-macOS E2E script/job;
- `docs/dev/brew-cask-homebrew-interop.md` decision results;
- a handoff state machine and rollback design;
- `plans/009-upstream-homebrew-registration-contract.md` scope correction;
- `plans/README.md` status.

**Out of scope**:

- mutation of the developer's real Homebrew state;
- blind use of `--adopt` for arbitrary casks;
- direct calls to private Ruby `Cask::Tab`;
- synthetic `.metadata` publication;
- production code before the decision gate;
- claiming cross-manager atomicity.

## Git workflow

- Branch: `test/brew-cask-native-handoff`
- Commit: `test(brew-cask): validate native ownership handoff`
- Use `git commit -s` and include
  `Co-authored-by: Codex <codex@openai.com>`.
- No upstream contact or PR without operator authorization. Any GitHub message
  must disclose that it was AI-generated.

## Steps

### Step 1: Freeze the exact Homebrew semantics

Against current upstream, document the operation order and failure cleanup for
`--adopt`, plain install, and `--force`. Record where app equality is checked,
where `auto_updates` skips it, how symlink artifacts qualify, when the installed
caskfile/tab become visible, and which staged files failure cleanup removes.

**Verify**: the result document contains commit-pinned links and a phase table
for all three modes.

### Step 2: Build isolated deterministic fixtures

Create a local tap and checksum-pinned archives with no network dependency for:

- moved `.app` with identical and different contents;
- `auto_updates` app with different contents;
- binary/symlink artifact;
- formula dependency plus binary;
- mixed app/binary;
- representative `pkg` and flight-hook cases that must fail eligibility.

Run only in a disposable macOS runner with isolated Homebrew prefix and appdir.
The script must assert its prefix/appdir are disposable before mutation, use a
scenario sentinel, and skip one scenario without exiting the suite.

**Verify**: fixture checksums are deterministic; the runner aborts before any
mutation when pointed at `/opt/homebrew`, `/usr/local`, or `/Applications`.

### Step 3: Exercise collision and failure matrices

For every eligible-looking fixture, test:

1. target exists, no Caskroom state;
2. matching mise target plus same-version mise Caskroom state;
3. differing target;
4. failure before stage, after stage, after target action, before tab, after tab;
5. retry after each failure;
6. `brew list --cask --versions`, upgrade/reinstall, uninstall, and mise status.

Capture exact filesystem and receipt trees before/after. Never accept command
exit 0 alone as proof.

**Verify**: each matrix row has expected owner, payload digest, Homebrew marker,
mise receipt, target, dependency, and retry result assertions.

### Step 4: Design reversible one-way handoff

The candidate flow is:

1. acquire mise's token lock and classify exact ownership;
2. reject conflicts and unsupported artifacts;
3. durably journal intended transition outside Homebrew-controlled token dirs;
4. quarantine/deactivate mise staging and targets to a same-volume recovery
   root without destroying the only good copy;
5. invoke one proven Homebrew mode;
6. verify Homebrew's lifecycle and exact payload result;
7. mark mise `Externalized`; mise refuses future mutation;
8. on failure, remove only newly created Homebrew state when proven safe and
   restore the mise snapshot.

This is one-way. Once Homebrew owns the cask, only Homebrew mutates it until a
separately designed explicit withdrawal completes.

**Verify**: crash diagrams name the linearization point and deterministic
recovery for every phase; no step assumes Homebrew honors mise's lock.

### Step 5: Apply the decision gate

Choose exactly one result:

- **Proven class-limited handoff**: production plan may expose explicit opt-in
  only for passing classes and exactness predicates.
- **Native reinstall only**: use Homebrew from the start or intentionally
  replace mise's payload; do not call it adoption/preservation.
- **Unsupported**: keep mise-owned mode only and narrow Plan 009 to the smallest
  missing supported capability.

Any `auto_updates`, same-version collision, rollback, or dependency ambiguity
excludes that class. A supported CLI name is not sufficient evidence.

**Verify**: a checked decision table maps every tested class to eligible,
ineligible, or unresolved with a failing/passing E2E reference.

## Test plan

- All fixture classes and three ownership starting states.
- Exact payload digest before/after, including `auto_updates` mismatch.
- Same-version Caskroom collision and failure cleanup.
- Dependency provenance and autoremove behavior.
- Post-handoff Homebrew list/upgrade/reinstall/uninstall.
- Post-handoff mise status is `Externalized` and refuses mutation.
- Scenario-executed sentinel; no zero-test pass.

## Done criteria

- [ ] Experiments ran only in an asserted disposable environment.
- [ ] Homebrew commit and every relevant semantic are pinned.
- [ ] `--adopt` is classified as full install, not registration.
- [ ] Every artifact class has evidence-based eligibility.
- [ ] Failure/retry and same-version collisions are covered.
- [ ] One product direction is selected by the decision gate.
- [ ] No production handoff shipped from an unresolved spike.

## STOP conditions

Stop if isolation cannot prevent real-prefix/app mutation; if a failure may
destroy the only payload copy; if test fixtures need arbitrary Ruby from an
untrusted source; if a lifecycle phase cannot be observed deterministically;
or if production work is requested before the matrix passes.

## Maintenance notes

Re-run this gate when Homebrew changes installer/adopt semantics. Never broaden
eligibility from app to binary/pkg/hooks by analogy; every class needs its own
complete lifecycle proof.
