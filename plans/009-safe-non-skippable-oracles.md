# Plan 009: Make destructive Homebrew oracles safe and non-skippable

Status: DONE
Priority: P0
Effort: M
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: none
Implementation commits: `dc37ee04a`, `459bb9d9b`, `3912e6fdc`,
`48782874f`, `2128bcf2b`, `94c66938c`, `84d74314f`, `df067e021`,
`a5046918a`, `12477a479`, `300551418`, `667866575`, `ce40d79d0`,
`671eedd2c`, `f373ab31e`, `8d6139272`, `a122a6601`, `7acd9be56`

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
