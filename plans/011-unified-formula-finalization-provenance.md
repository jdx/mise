# Plan 011: Unify formula finalization and preserve truthful provenance

Status: DONE
Priority: P0
Effort: L
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: 010
Implementation commits: `a7f2352e1`, `43cc1d8c1`, `227664100`, `7cebd5e51`,
`b9a82d4fb`, `a2697d00f`, `ca2438834`, `09bccf0a7`, `56ef7caf1`,
`75bdcadbd`, `c9390008a`, `38b25fa5b`, `93df3357c`, `7c5374c0a`

Post-review correction (2026-08-15):

- [#11915 review thread](https://github.com/jdx/mise/pull/11915#discussion_r3789746144)
  identified that matching a live shared file against every stale keg default
  can overwrite user-selected configuration that happens to equal a
  non-predecessor default;
- formula finalization now captures the exact active keg before any keg/link
  mutation and compares persistent defaults only against that predecessor;
- same-version replacement remaps the captured keg path to the transaction
  backup. Missing or unprovable predecessor state takes the conservative
  `.default` path instead of inferring ownership from arbitrary old kegs;
- focused compile and hosted exact-head proof pass. The review thread is
  resolved with the fixing commit and exact-head evidence.

Exact-predecessor completion proof:

- exact #11915 head `7cebd5e51141c7ea97eb1f5febb32cc5ae465fa4`
  passes the complete
  [test workflow](https://github.com/jdx/mise/actions/runs/31896105424),
  including Linux, macOS, Windows, lint/Clippy, unit, e2e, bootstrap, and the
  aggregate gate;
- [macOS oracle job 95039581724](https://github.com/jdx/mise/actions/runs/31896105424/job/95039581724)
  records cask/formula counts `1`/`4` at `/opt/homebrew`. Both markers bind the
  exact mise head and matching Homebrew reference/runtime `6.0.17` at
  `4dacfe77a24dead72de749c0876028b77b99cd04`;
- [Linux/source oracle job 95039581687](https://github.com/jdx/mise/actions/runs/31896105424/job/95039581687)
  records counts `1`/`1` at `/home/linuxbrew/.linuxbrew` and
  `/tmp/mise-brew-source-prefix`. Those intentionally brew-free bodies record
  runtime Homebrew as `not-installed` while retaining the exact reference;
- [macOS unit job 95039581757](https://github.com/jdx/mise/actions/runs/31896105424/job/95039581757)
  executes the regressions proving that a stale non-predecessor default cannot
  authorize overwrite and that same-version replacement compares against the
  transaction backup containing the exact predecessor.

Interrupted-finalization review correction (2026-08-20):

- review threads [3814465247](https://github.com/jdx/mise/pull/11915#discussion_r3814465247)
  and [3814704826](https://github.com/jdx/mise/pull/11915#discussion_r3814704826)
  showed that retry reconstructed predecessor state from the newly active keg,
  so an interruption after link mutation could lose the original predecessor;
- commit `b9a82d4fb3292a24041224965b7f38f269ea5f3d` persists the original
  predecessor in the durable finalization journal, validates transaction
  identity, and reuses a deterministic recovery backup only when a matching
  incomplete journal proves ownership;
- commit `a2697d00f77fa535b140a951f94a9cfeea68b06a` closes the retry state
  machine: absent progress retries with the durable predecessor, complete
  progress commits cleanup without replay, and incomplete or unknown progress
  fails before mutation. Shared-state completion is recorded explicitly and
  source finalization performs the same pre-check;
- five focused finalizer regressions, all 310 formula-stack brew tests, and full
  workspace/all-feature/all-target Clippy with `-D warnings` pass locally. Both
  review threads are resolved. Exact implementation
  [run 32280764818](https://github.com/jdx/mise/actions/runs/32280764818)
  executed both authenticated Homebrew oracles on
  `855602c94410343ead04d36abccc4e0f3407474d`; their four completion markers
  bind that exact head and the pinned Homebrew source. Signed merge
  `b6f4074ed3d8752fb94a9456d7e2e74a7dedd96b` then integrates canonical main
  `b7c9cbaa0320a70d63ec3fe69aa86ea604839b00` without touching finalizer or
  lifecycle code. Signed merge `e5fc0f5f7d1791a2ee0b1b157e1c3c6eed15fd80`
  subsequently integrates canonical main
  `f7ad18b4b00047af872d58fa571bbf412c24be83`; its release/pacman delta is also
  disjoint from brew code.

Final shared-state provenance correction (2026-08-20): commit `ca2438834`
persists the exact `.bottle/etc|var` source to installed target mapping. This
retains the authoritative `.default` destination selected when user-modified
configuration is preserved; lifecycle repair can recreate a deleted default
without repouring or changing the user file, keg, native receipt, or public
link. The same correction accepts a valid Homebrew directory-level public link
only when resolving its ancestor plus leaf suffix reaches the exact expected
keg path; another version or subtree remains a hard conflict.
Follow-up `09bccf0a7` constructs `.default` paths from raw platform path bytes
instead of display-formatted text, preserving non-UTF-8 path identity while
satisfying strict Clippy without exclusions.

Run [32285796215](https://github.com/jdx/mise/actions/runs/32285796215)
completed all 13 jobs on `e5fc0f5f7d1791a2ee0b1b157e1c3c6eed15fd80`,
including both authenticated formula/source oracles. It is historical proof,
not final closure, because the subsequent authority audit found finalization,
topology, source-confinement, and removal gaps.

Final authority correction `56ef7caf16a5e660ce62e016acf65fa001f1433d`
binds finalization and lifecycle plans to exact receipt/snapshot/incarnation
identity; persists original rollback state and full public-topology journals;
quiesces same-version links before build/replacement; validates full real
directory ancestry; and stages bottle/source work under unique identity-bound
roots with bounded cleanup. Resume accepts only the exact durable phase,
predecessor, lifecycle digest, and filesystem identities.

Linux source execution is environment-, filesystem-, network-, local-socket-,
and process-group-confined. Formula API names, versions, tap identity, checksums,
install policy, and the supported Ruby DSL are validated before mutation.
Unknown DSL and install-affecting metadata fail closed. macOS remains
bottle-only: source builds fail before download or Cellar mutation because
`sandbox-exec` cannot prove cleanup of a deliberately detached descendant.
This is an explicit platform boundary, not a false equivalence claim.

Signed merge `75bdcadbdec7e2b93d50409e4f3d4fa9e319e655` integrates canonical
`origin/main` `b0795afa7d23de9eed01b3f82cde5789830fc550` without conflict. Exact
[run 32298940105](https://github.com/jdx/mise/actions/runs/32298940105) proved
the macOS cask and Linux formula paths, then failed the macOS lifecycle oracle
on exact shared-output parent creation and the Linux source oracle because its
Docker environment could not enforce mandatory Landlock. It is failure
evidence, not closure.

Correction `c9390008a07ca71797b93c158e6ef5a9bff0e0ae` preserves exact
shared-output authority while creating required real parents and moves the
source oracle to the fresh Linux host where the same production command must
successfully enforce Landlock, seccomp, local-socket denial, and process-group
cleanup before a marker can be emitted. Local proof passes 392 brew tests,
3,525 workspace/all-feature tests with four ignored, native-Linux sandbox/cmd
suites `22`/`27`, strict workspace/all-target Clippy, formatting, and diff
checks. Exact hosted
[run 32302254890](https://github.com/jdx/mise/actions/runs/32302254890) on that
head failed and remains negative evidence: the macOS lifecycle helper still
required an adjacent temporary directory denied by the sandbox, the Linux
source guard rejected the host's installed Homebrew against its declared
`not-installed` runtime, and no marker was emitted for either body. Unit,
Windows unused-code, and aggregate failures independently prevent closure.

Security correction `38b25fa5b71da407a82ae12600896f2f85f1ed96`
binds lifecycle and source effects to typed filesystem identities and retained
transactional rollback authority. Shared regular-file outputs are built in a
private mirror and published transactionally; strict Linux
Landlock/seccomp/local-socket/process containment is mandatory. The only macOS
formula Run exception is the exact pinned and audited `ca-certificates` recipe
and helper, executed from verified private copies; drift fails closed before
publication. Local exact-lineage proof passes 87 lifecycle tests, all 423 brew
tests, strict workspace/all-feature/all-target Clippy, formatting, and diff
checks.

Oracle-provenance correction `93df3357c6be57fae0ceda4aa03973ad8d2c407e`
separates Linux formula and source execution, pins the disposable executor by
digest, removes CI credentials, and authenticates exact-head job context and
completion markers before aggregation. Signed merge
`7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358` integrates canonical
`origin/main` `60c5ff113a672269c1bd9455f4eb50a079371a17`; its docs, registry,
and Aqua changes do not overlap formula finalization or lifecycle code.

Exact-head [run 32312879235](https://github.com/jdx/mise/actions/runs/32312879235)
for `7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358` failed and remains negative
evidence. Linux formula completed; macOS lifecycle rejected a live-API formula
checksum that differed from the checksum-verified bottle snapshot; Linux
source rejected GNU `install` metadata mutation under strict confinement;
nightly portability and Linux Clippy gates also failed. The aggregate correctly
failed. Restore `DONE` only after a later exact head's complete workflow,
authenticated formula/source markers, and aggregate gate pass.

## Objective

Give OCI bottles, archive bottles, and source builds one ordered finalization
state machine while preserving source-specific facts. Every successful formula
must receive receipt/SBOM, link state, `install_etc_var`, and required typed
post-install behavior exactly once.

## Current defect

- #11915 bottle path calls lifecycle install; source path writes receipt and
  links but never calls it.
- #11910 source receipt derives facts from `<keg>/.brew/<name>.rb`, but the
  verified formula source is never copied there. Receipt generation fails and
  removes the built keg.
- Non-OCI bottles return `None` from fetch, then `pour.rs` calls
  `source_build_facts`, consults local compiler/build-host facts, and may require
  `cc` merely to pour a bottle.
- Formula linking deliberately omits `etc` and `var`; without the shared-state
  lifecycle, OpenSSL/CA state remains trapped under `.bottle`.

## Files in scope

- `src/system/packages/brew/fetch.rs`
- `src/system/packages/brew/pour.rs`
- `src/system/packages/brew/source.rs`
- `src/system/packages/brew/receipt.rs`
- `src/system/packages/brew/lifecycle.rs`
- relevant API/shim modules and focused tests

## Required architecture

Replace overloaded `Option` provenance with a closed input enum such as:

- `OciBottle { verified_manifest, receipt, sbom }`
- `ArchiveBottle { verified_archive_receipt, embedded_sbom }`
- `SourceBuild { verified_formula_snapshot, build_facts }`

The common finalizer accepts the staged keg, prepared lifecycle from plan 010,
and this provenance. It owns ordered transitions and durable failure state.
Input-kind-specific extraction happens before public-link/shared-state mutation.

Required order must be verified against pinned Homebrew 6.0.17. At minimum:
receipt/SBOM and keg finalization, public linking/linked-keg state,
`install_etc_var`, then post-install. A post-install failure must never be
reported `Installed`; it must retain enough phase data for plan 012 repair.

## Implementation steps

1. Parse an archive bottle's embedded receipt/SBOM from the checksum-verified
   archive before overwriting any metadata. Validate schema and formula/version
   identity. Missing required facts fail as an archive-bottle error, never fall
   through to source facts.
2. For source builds, write the already verified formula text atomically to
   `<keg>/.brew/<name>.rb` before receipt derivation. Receipt timestamp,
   compiler, source URL/revision, and build host must reflect actual build data.
3. Introduce the explicit provenance enum and remove `None` as a semantic
   discriminator. Exhaustive matching must make new sources fail at compile
   time until provenance is defined.
4. Extract one common finalizer used by both `pour` and `source`. It consumes the
   plan 010 prepared lifecycle and records each completed phase durably.
5. Implement Homebrew `install_etc_var` semantics, not ordinary public links:
   migrate `.bottle/etc` and `.bottle/var` defaults to shared prefix paths;
   create missing defaults/directories; preserve user-modified existing files;
   resolve conflicts/backups like pinned Homebrew; keep version/default identity
   sufficient for later upgrade comparison; stage or journal every mutation.
6. Distinguish source-build shared outputs from bottle `.bottle` roots. Never
   copy a source default back over configuration that the formula installation
   already created.
7. Use the same post-install execution path for all three provenance kinds.
8. Consolidate failure cleanup. Never delete a pre-existing valid keg on a
   finalization error. Newly staged kegs may be rolled back only when all shared
   effects are also proven reversible.

## Required tests

- Forced-source formula with shared `etc`, `var`, and a nontrivial typed
  post-install: formula snapshot exists, receipt parses, lifecycle output exists.
- Source failure before and after shared-state mutation: no false Installed and
  no lost pre-existing config.
- Non-OCI bottle with embedded receipt and no `cc` on PATH: pour succeeds with
  preserved bottle build/compiler facts.
- Malformed/missing archive receipt: fails before public mutation.
- OCI and archive fixtures produce semantically equivalent Homebrew receipts and
  SBOMs where their authoritative input facts match.
- Upgrade default unchanged, user modified, removed upstream, and type conflict
  cases for both `etc` and `var`.
- `ca-certificates`/`openssl@3` fixture creates shared defaults without package
  name special cases.

## Verification

Completed proof:

- Exact-head Linux source execution at
  `https://github.com/jdx/mise/actions/runs/31648682993/job/94288067860`
  compiled GNU hello successfully, then exposed that Ubuntu identifies GCC as
  `cc (Ubuntu ...)` rather than including the literal `gcc`. Commit
  `8f28b453847133bc6cc097963b7a62a970c8985c` recognizes the authoritative GNU
  banner and records Homebrew-compatible `gcc-<major>` from the compiler's
  version probe; focused source tests pass.

- `rtk cargo test --bin mise system::packages::brew` — 204 passed at the
  formula head.
- `rtk cargo test --bin mise archive_bottle` — 2 passed; malformed/missing
  embedded receipt fails and archive provenance never queries local compiler.
- `rtk cargo test --bin mise source_receipt_requires_snapshot` — 1 passed;
  source receipt requires the atomic verified formula snapshot and writes its
  source SBOM.
- The exact-head macOS canonical lifecycle job above exercised OCI bottle
  finalization through receipt/link/shared-state/post-install/health. The full
  Linux e2e job owns the forced-source script; pre-integration branch CI is not
  final combined proof.
- Commit `227664100` selects an `all` bottle's host-tagged SBOM supplement,
  matching pinned Homebrew instead of serializing the whole supplement map.
- Current-main #11915 head `7009878b784c4ee3436d365efd2693fb4c909e50`
  passed the complete [test workflow](https://github.com/jdx/mise/actions/runs/31735998370).
  Its [macOS oracle](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757793)
  proved OCI finalization at `/opt/homebrew`; its
  [Linux oracle](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757706)
  proved archive/Linux formula and forced-source paths. Exact markers bind
  fixture counts `4`, `1`, and `1` to the tested mise head and pinned Homebrew
  `6.0.17` / `4dacfe77a24dead72de749c0876028b77b99cd04`.

```bash
rtk cargo test --bin mise system::packages::brew
rtk mise run test:e2e e2e/cli/test_system_install_brew_source_slow
rtk mise run test:e2e e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow
rtk mise run lint
```

## Done criteria

Final closure: exact code head `be74f2563308dcc1ea6628bbcb7364fd124a64f0`
passed the complete workflow in [run 32339247676](https://github.com/jdx/mise/actions/runs/32339247676),
including bottle and source provenance, receipt, formula snapshot, SBOM,
linked-record, and completion-marker checks.

- Bottle/archive/source cannot be confused in provenance code.
- Source builds preserve the verified formula snapshot and execute lifecycle.
- Archive bottles never query local compiler facts.
- Shared `etc`/`var` behavior preserves user modifications across upgrades.
- A formula reaches Installed only after all required finalization phases.

## Stop conditions

Never synthesize missing bottle build facts. Never overwrite an existing shared
file whose ownership/content history is unproven. Return an exact conflict or
reinstall requirement instead.
