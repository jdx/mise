# Plan 011: Unify formula finalization and preserve truthful provenance

Status: DONE
Priority: P0
Effort: L
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: 010
Implementation commits: `a7f2352e1`, `43cc1d8c1`

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

```bash
rtk cargo test --bin mise system::packages::brew
rtk mise run test:e2e e2e/cli/test_system_install_brew_source_slow
rtk mise run test:e2e e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow
rtk mise run lint
```

## Done criteria

- Bottle/archive/source cannot be confused in provenance code.
- Source builds preserve the verified formula snapshot and execute lifecycle.
- Archive bottles never query local compiler facts.
- Shared `etc`/`var` behavior preserves user modifications across upgrades.
- A formula reaches Installed only after all required finalization phases.

## Stop conditions

Never synthesize missing bottle build facts. Never overwrite an existing shared
file whose ownership/content history is unproven. Return an exact conflict or
reinstall requirement instead.
