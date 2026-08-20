# Plan 009: Make destructive Homebrew oracles safe and non-skippable

Status: DONE
Priority: P0

Current exact-head confirmation (2026-08-20): prerequisite head
`7d422518ed1674c8cb5f819411af9bee4d4b6ce6` passed all 14 jobs in
[run 32352460903](https://github.com/jdx/mise/actions/runs/32352460903),
including authenticated macOS, Linux bottle, and isolated Linux source
completion-marker verification.
Effort: M
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: none
Implementation commits: `dc37ee04a`, `459bb9d9b`, `3912e6fdc`,
`48782874f`, `2128bcf2b`, `94c66938c`, `84d74314f`, `df067e021`,
`a5046918a`, `12477a479`, `300551418`, `667866575`, `ce40d79d0`,
`671eedd2c`, `f373ab31e`, `8d6139272`, `a122a6601`, `7acd9be56`,
`855602c94`, `1b821d2fc`, `56ef7caf1`, `75bdcadbd`, `c9390008a`,
`38b25fa5b`, `93df3357c`, `7c5374c0a`

## Objective

Replace the false-green `CI=true` guards with an explicit capability for a
known-disposable runner. Prove every dedicated workflow entered and completed
the intended oracle body. This plan repairs test infrastructure; it must not
weaken failing feature assertions merely to recover green CI.

## Current defect

- `e2e/run_test:74-126` launches tests through `env -i`. `CI` and
  `MISE_BREW_LIFECYCLE_TEST_PREFIX` are not forwarded.
- The macOS and Linux scripts exit zero when `CI != true`.
- GitHub jobs `93970283233` and `93980178865` therefore reported success in
  zero seconds without exercising fixtures.
- Forwarding generic `CI` is unsafe: those scripts uninstall apps/fonts or
  delete `/home/linuxbrew` and a user.
- #11915's formula lifecycle script is absent from the macOS workflow, silently
  succeeds when its chosen prefix exists, and reaches default Node CA checks
  only for `/opt/homebrew`.

## Files in scope

- `e2e/run_test`
- `e2e/cli/test_system_install_brew_macos_slow`
- `e2e/cli/test_system_install_brew_linux`
- `e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow`
- `.github/workflows/test.yml`
- focused harness tests or helpers under `e2e/`

Do not change package installation behavior here.

## Required design

1. Define one private capability, e.g.
   `MISE_BREW_ORACLE_DISPOSABLE=1`. It is authorization, not an ordinary test
   selector. Forward it through `env -i` only for named brew-oracle tests.
2. Forward a result directory rooted under `RUNNER_TEMP`, e.g.
   `MISE_BREW_ORACLE_RESULT_DIR`. Reject empty, relative, symlink-escaped, or
   non-runner paths before mutation.
3. The workflow sets the capability only in dedicated disposable jobs. Normal
   e2e jobs and local `CI=true` runs do not receive it.
4. On the target platform, missing capability or ambiguous prerequisites is a
   hard nonzero failure. On an irrelevant platform, print an explicit skip
   reason; dedicated jobs must fail if their completion marker is absent.
5. Each oracle writes a unique completion record only after all assertions,
   containing the test name, fixture count, target prefix, and exact mise SHA.
   The workflow checks that record and a positive fixture count in a separate
   step that runs even if the test command unexpectedly returns zero early.
6. Never infer disposability solely from `CI`, `GITHUB_ACTIONS`, root, hostname,
   or the existence of a temp directory.

## Implementation steps

1. Add a small harness policy function for allowlisted environment forwarding.
   Unit/e2e-test it with a probe: capability and result directory survive for a
   brew oracle; `CI` and arbitrary variables remain stripped.
2. Centralize the destructive preflight used by all three scripts. Before any
   cleanup, verify platform, explicit capability, exact expected prefix, result
   directory containment, and test-specific ownership conditions.
3. Linux: require a fresh dedicated runner and fail if `/home/linuxbrew`, its
   intended user, or foreign matching processes already exist. Never delete
   pre-existing ambiguous state.
4. macOS casks: inventory exact fixture tokens and public targets first. Fail if
   any target cannot be proven to belong to the fixture setup. Cleanup must be
   bounded to those exact paths.
5. Formula canonical-prefix job: invoke the exact lifecycle test explicitly.
   The job must deliberately prepare disposable `/opt/homebrew` state and record
   what it displaced. Never silently fall back to `/tmp/misebrew` for canonical
   bottle/runtime claims.
6. Remove `exit 0` on stale prefix. A pre-existing unexpected prefix is an
   unsafe-state failure; an expected prepared prefix proceeds.
7. Add workflow marker validation. Upload logs/normalized snapshots on failure.
8. Trigger both dedicated jobs. Preserve red feature failures as evidence for
   plans 010–018; do not call this plan done until marker validation proves the
   intended bodies ran.

## Verification

### Exact-head execution evidence

- Formula head `57319487b35414076982cd1828c1114b3dddeea5`, workflow run
  `31644065133`: the Linux body executed one fixture and all topology, runtime,
  repair, import, and prune assertions passed. Marker verification then failed
  because the root container created the bind-mounted marker as mode `0600`;
  artifact upload independently failed with `EACCES`. The harness now passes
  only the host numeric result owner to the named Linux oracle, atomically
  transfers the completed marker to that owner, and makes it read-only to
  ordinary workflow consumers (`0644`). Invalid owner data fails without a
  marker.
- The same run's macOS body entered the cask fixture and failed nonzero on an
  upstream GitHub HTTP/2 refused stream before any completion marker. The
  always-run verifier correctly rejected the missing marker. This is retained
  as a real failed execution, not counted as proof.
- Local guard proof after the ownership correction:
  `rtk mise run test:e2e e2e/cli/test_brew_oracle_guard` — PASS.
- Final #11915 proof head `84d74314f904c0accad1c4cdc563ea662360316b`:
  [macOS job](https://github.com/jdx/mise/actions/runs/31645663032/job/94278577725)
  and [Linux job](https://github.com/jdx/mise/actions/runs/31645663032/job/94278577719)
  both PASS. Separate verifier steps accepted all three exact markers and
  failure artifacts uploaded successfully.
- Marker identities: macOS cask `fixture_count=1`, macOS lifecycle
  `fixture_count=4`, Linux formula `fixture_count=1`; all record exact mise
  head, reference Homebrew `6.0.17` / `4dacfe77a24dead72de749c0876028b77b99cd04`,
  runtime identity (`5.1.4` / `968378a218ba485371a8a849185f05a8b534bda7`
  on macOS, explicit `not-installed` on Linux), and canonical tested prefix.

### Current-main exact-head proof

After restacking on `origin/main` `cf40f5c605ef77693f69c766aa3642fd78464cc7`,
PR #11915 exact head `7009878b784c4ee3436d365efd2693fb4c909e50`
passed the complete [test workflow](https://github.com/jdx/mise/actions/runs/31735998370).
The dedicated [macOS oracle job](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757793)
validated cask fixture count `1` and lifecycle fixture count `4`; the dedicated
[Linux oracle job](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757706)
validated formula and source fixture counts `1` each. All four markers record
the exact mise head, Homebrew reference `6.0.17` at
`4dacfe77a24dead72de749c0876028b77b99cd04`, and their tested prefix. macOS
also records the same exact runtime Homebrew version/SHA; Linux records the
intentional runtime value `not-installed`. The workflow's aggregate `test-ci`
gate passed in [job 94574628303](https://github.com/jdx/mise/actions/runs/31735998370/job/94574628303).

Latest-main revalidation: merge commit `b112975e0d6858c2b872259970f84b4002bc9d5e`
integrated `origin/main` `619854b468dd3fffe0d475a08d69e4c82da80acd`.
Oracle compatibility corrections are `58e26829048d29f09d749327a8dcca15cc862f36`
and `1c3ce7cecb049a198fb64a658b9389cdbe9241d6`. Exact implementation head
`1c3ce7cecb049a198fb64a658b9389cdbe9241d6` passed the complete
[test workflow](https://github.com/jdx/mise/actions/runs/32275082730), including
the [macOS oracle](https://github.com/jdx/mise/actions/runs/32275082730/job/96140855866)
and [Linux/source oracle](https://github.com/jdx/mise/actions/runs/32275082730/job/96140855969).
The validated markers record macOS cask/lifecycle fixture counts `1`/`5` and
Linux formula/source counts `1`/`1`, exact mise head, and Homebrew `6.0.17` at
`4dacfe77a24dead72de749c0876028b77b99cd04`; only isolated Linux source
intentionally records runtime `not-installed`.

Credential-isolation correction (2026-08-20): commit `855602c94410343ead04d36abccc4e0f3407474d`
clears `GITHUB_TOKEN`, `GH_TOKEN`, and `MISE_GITHUB_TOKEN` from both destructive
oracle jobs, disables persisted checkout credentials, and makes the common
disposable-runner preflight reject leaked GitHub credentials before mutation.
The guard E2E sets a token deliberately and proves this refusal path. Exact
implementation [run 32280764818](https://github.com/jdx/mise/actions/runs/32280764818)
executed the corrected jobs: the
[macOS oracle](https://github.com/jdx/mise/actions/runs/32280764818/job/96159101336)
emitted cask/lifecycle counts `1`/`5`, and the
[Linux/source oracle](https://github.com/jdx/mise/actions/runs/32280764818/job/96159101396)
emitted counts `1`/`1`. All four markers bind exact implementation head
`855602c94410343ead04d36abccc4e0f3407474d` and Homebrew `6.0.17` at
`4dacfe77a24dead72de749c0876028b77b99cd04`; only isolated source records
runtime `not-installed`.

Latest-main refresh: signed merges `b6f4074ed3d8752fb94a9456d7e2e74a7dedd96b`
and `e5fc0f5f7d1791a2ee0b1b157e1c3c6eed15fd80` integrate canonical
`origin/main` through `f7ad18b4b00047af872d58fa571bbf412c24be83`. The
upstream deltas are release, registry, pacman, vendor, completion, and generated
metadata; they do not overlap brew production, oracle, or plan code. The
following plan closure commit owns the final exact-head hosted suite.

General-E2E correction: although both authenticated oracle jobs and their four
markers passed in run `32280764818`, the complete workflow was not green. Its
generic E2E job inherited `MISE_GITHUB_TOKEN`; the guard self-test cleared only
the token it injected, so both retries failed before the positive preflight.
Commit `1b821d2fc` makes the test environment deterministic by clearing all
three GitHub token variables first, then independently proving that
`GITHUB_TOKEN`, `GH_TOKEN`, and `MISE_GITHUB_TOKEN` each block destructive
authorization. The production guard is unchanged and remains fail-closed.
`rtk mise run test:e2e e2e/cli/test_brew_oracle_guard`, shellcheck, and shfmt
pass locally.

Run [32285796215](https://github.com/jdx/mise/actions/runs/32285796215)
subsequently completed all 13 jobs on `e5fc0f5f7d1791a2ee0b1b157e1c3c6eed15fd80`;
both authenticated oracles and all four exact-head markers passed. That run is
retained as valid historical evidence, but not final closure: the following
ownership audit found product authority gaps that required a new implementation
head.

Final ownership hardening is commit `56ef7caf16a5e660ce62e016acf65fa001f1433d`.
Signed merge `75bdcadbdec7e2b93d50409e4f3d4fa9e319e655` integrates canonical
`origin/main` `b0795afa7d23de9eed01b3f82cde5789830fc550` without conflict. Exact
[run 32298940105](https://github.com/jdx/mise/actions/runs/32298940105) is
retained as negative evidence: only the macOS cask and Linux formula markers
completed. The macOS lifecycle oracle exposed missing exact-parent creation for
a declared shared output; the Linux source oracle exposed that its disposable
Docker environment could not enforce mandatory Landlock; Windows found two
Unix-only cleanup calls; and unit/nightly jobs found a stale Ruby fixture plus
test-child cwd reinitialization.

Correction `c9390008a07ca71797b93c158e6ef5a9bff0e0ae` creates only declared
shared-output parents with identity-bound rollback, reports unavailable
Landlock before spawn, runs the source oracle directly on the fresh Linux host,
and makes both brew oracle jobs mandatory dependencies of `test-ci`. It also
fixes the Windows gates and isolates re-executed test children. Local proof on
that exact head passes 392 brew tests, 3,525 workspace/all-feature tests with
four ignored, native-Linux sandbox/cmd suites `22`/`27`, strict
workspace/all-target Clippy, formatting, Actionlint, ShellCheck, Ruby 4 syntax,
diff checks, and the oracle-guard E2E. Exact hosted
[run 32302254890](https://github.com/jdx/mise/actions/runs/32302254890) on that
head failed and is retained as negative evidence, not closure. The macOS
lifecycle body could not create the adjacent temporary directory required by
the audited `ca-certificates` helper; the Linux source body was rejected because
Homebrew was present despite a `not-installed` runtime declaration. Nightly and
macOS unit tests, the Windows unused-code gate, and the aggregate also failed.
The macOS lifecycle and Linux source completion markers were therefore absent.

Security correction `38b25fa5b71da407a82ae12600896f2f85f1ed96`
supersedes the incomplete pathname-parent scheme. Formula execution and typed
lifecycle effects now bind exact filesystem identities, retain transactional
rollback authority, stage shared regular-file outputs privately, and fail
closed unless strict Linux Landlock/seccomp/process containment is available.
The audited macOS `ca-certificates` helper is pinned and executed from a verified
private copy; generic uncontained macOS formula execution remains rejected.
Local exact-lineage proof passes 87 lifecycle tests, all 423 brew tests, strict
workspace/all-feature/all-target Clippy, formatting, and diff checks.

Oracle-provenance correction `93df3357c6be57fae0ceda4aa03973ad8d2c407e`
splits Linux formula and source proof across capable environments, pins the
disposable image by digest, strips every supported CI credential, and binds job
context plus completion markers to the exact head, platform, Homebrew reference
and runtime, executor identity, and declared test set. Missing or mismatched
evidence fails the aggregate. Signed merge
`7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358` then integrates canonical
`origin/main` `60c5ff113a672269c1bd9455f4eb50a079371a17`; that docs, registry,
and Aqua delta does not overlap brew implementation or oracle code.

Exact-head [run 32312879235](https://github.com/jdx/mise/actions/runs/32312879235)
for `7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358` failed and remains negative
evidence. Linux formula completed with an authenticated marker; macOS cask
completed, but formula lifecycle rejected mutable API snapshot drift before its
marker; Linux source rejected GNU `install` metadata mutation; nightly exposed
strict-sandbox test portability; Linux Clippy found four platform conversions.
The aggregate correctly failed. Restore `DONE` only after a later exact head's
complete workflow, all authenticated markers, and aggregate gate pass.

```bash
rtk mise run test:e2e e2e/cli/test_system_install_brew_macos_slow
rtk mise run test:e2e e2e/cli/test_system_install_brew_linux
rtk mise run test:e2e e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow
rtk mise run lint
```

Local destructive tests without the capability must fail before mutation. Run
the real bodies only in the dedicated disposable workflow. Inspect logs for
fixture commands and marker contents; elapsed time alone is not evidence.

## Done criteria

Final closure: exact code head `be74f2563308dcc1ea6628bbcb7364fd124a64f0`
passed the complete workflow in [run 32339247676](https://github.com/jdx/mise/actions/runs/32339247676),
including credential-cleared macOS, Linux bottle, and Linux source jobs with
exact completion-marker verification.

- Dedicated jobs name every intended oracle, and every job validates a unique
  completion marker with a nonzero fixture count.
- A regression test proves `env -i` forwards only the private capability data.
- Missing sentinel, stale/foreign target, wrong prefix, or missing marker fails.
- Generic `CI=true` cannot authorize cleanup.
- #11915 formula lifecycle test is invoked at canonical prefix.
- Feature-level red results are documented, never converted to skips.

## Stop conditions

Stop before mutation if disposability cannot be proven from explicit job-owned
state. Do not test on an operator Mac or persistent Linux host. Do not broaden
the environment allowlist to make unrelated tests pass.
