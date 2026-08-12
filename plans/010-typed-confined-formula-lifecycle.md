# Plan 010: Compile formula lifecycle once, preflight it, and confine it

Status: DONE
Priority: P0
Effort: L
Planned against: #11915 `b94b6b1c1`
Depends on: 009
Implementation start: #11915 `4989ac953`
Implementation commits: `4989ac953`, `43cc1d8c1`, `57319487b`

Drift check (2026-08-13): the typed preparation and confined executor are now
present. Exact-head macOS proof exposed one ordered-effect defect: Node removes
the old npm tree and recreates it later, while lifecycle health retained the
intermediate removal as a permanent absence invariant. Final-state effects must
be folded in execution order. The Linux oracle also self-matched because its
process command-line scan contained the canonical prefix literal.

## Objective

Turn API lifecycle metadata into one closed, validated execution plan before
any formula mutation. Execute that same value under Homebrew-equivalent
confinement. Unsupported semantics must reject only formulae that will actually
be mutated.

## Current defect

- `brew/mod.rs:88-110` validates the whole dependency closure before computing
  `to_pour`. An already-current formula with unsupported lifecycle metadata can
  block unrelated work. Current `postgresql@17` is a concrete case: its metadata
  uses `link_dir`, `link_children`, and `init_data_dir`.
- `lifecycle.rs:32-88` shallow-validates raw JSON; `:351-480` reparses it through
  a second string dispatch. Unsupported bases/templates fail at `:600-647`,
  after keg linking.
- Brace expansion handles one group. Recursive remove uses `Path::exists`, so a
  dangling symlink can survive.
- `run` executes directly with inherited environment and privileges. Homebrew
  6.0.17 runs post-install through its sandbox on macOS.

## Files in scope

- `src/system/packages/brew/lifecycle.rs`
- `src/system/packages/brew/api.rs`
- `src/system/packages/brew/mod.rs`
- existing sandbox/command-runner modules only where needed to reuse their
  confinement primitives
- lifecycle unit fixtures and focused e2e tests

No package-specific OpenSSL or CA hard-coding. No new public CLI/config.

## Required architecture

Create a `PreparedFormulaLifecycle` (name may differ) containing typed ordered
steps. Each step is a closed enum with typed paths, guards, mode, arguments,
environment, working directory, redirections, and rollback/ownership metadata.
Deserialize with unknown-field rejection where the upstream schema is closed.
Keep the raw API only for diagnostics; execution never reparses it.

Preparation has no side effects and resolves:

- every path base and template;
- all brace expansions, including multiple groups;
- platform guards and `unless_exists` semantics;
- canonicalized lexical containment against allowed roots without requiring a
  target to exist;
- executable identity, argv, cwd, environment, network requirement, input and
  output paths;
- whether each effect is reversible, idempotent, and health-checkable.

No `unreachable!` may stand in for upstream validation.

## Implementation steps

1. Compute closure state first: current/healthy, lifecycle repair candidate, or
   full pour. Build the complete mutation set before validation.
2. Prepare lifecycle metadata for all and only formulae in that mutation set.
   If any is unsupported, fail the whole preflight before the first mutation.
3. Replace raw-map validator/executor pairs with one typed parser and executor.
   Error messages include formula, step index/type, unsupported field/value, and
   zero-mutation guarantee.
   Retain the fail-closed boundary between typed `post_install_steps` and opaque
   Ruby `post_install`: when authoritative metadata says `post_install_defined`
   but does not provide a complete typed representation, reject before mutation.
   Never interpret an empty typed list as proof that an opaque hook is empty.
4. Fix lexical path handling for dangling symlinks. Recursive deletion operates
   on `symlink_metadata`, never follows a link outside allowed roots, and records
   the exact owned node.
5. Reuse repository `SandboxConfig`/command-runner confinement. Construct a
   minimal deterministic environment and explicit write allowlist for the keg,
   allowed shared `etc`/`var`, logs, cache/temp, and documented toolchain paths.
   Network is denied unless authoritative lifecycle metadata requires it.
6. On macOS, match Homebrew's sandbox boundary closely enough for the same
   fixture to pass/fail in both engines. On platforms where equivalent
   confinement cannot be established, fail closed for `run` before mutation.
7. Capture stdout/stderr in formula post-install logs with bounded diagnostic
   reporting. Do not inherit secrets or unrelated environment variables.

## Required tests

- Already-current `postgresql@17`-shaped dependency plus unrelated formula to
  pour: current dependency is not rejected or touched.
- Same unsupported lifecycle on a formula in the mutation set: fails before
  keg/link/etc/var changes.
- Unknown key/type/base/template and invalid cwd/redirection: preflight failure.
- `post_install_defined=true` without complete typed steps: preflight failure.
- Multiple brace groups; dangling symlink remove; traversal and symlink escape.
- Valid `unless_exists` and platform guard behavior.
- A `run` fixture writes inside each allowed root and succeeds.
- The same fixture attempts a write outside allowed roots and network access;
  confinement blocks it and leaves no external file.
- Sanitized child environment contains only the documented keys.

## Verification

Completed proof:

- `rtk cargo test --bin mise system::packages::brew::lifecycle` — 15 passed.
- `rtk cargo test --bin mise system::packages::brew` — 204 passed at the
  formula head.
- Exact-head macOS lifecycle oracle at `84d74314f904c0accad1c4cdc563ea662360316b`
  passed in [job 94278577725](https://github.com/jdx/mise/actions/runs/31645663032/job/94278577725).
- The ordered-effect regression proves a removed path recreated by a later
  typed step is healthy. Linux process-reference safety no longer self-matches.
- Confinement tests cover deterministic environment, allowed roots, traversal,
  symlink escape, network denial, and outside-root write denial. Unsupported
  lifecycle on a non-mutating current dependency is not validated; the complete
  mutation set is validated before its first side effect.

```bash
rtk cargo test --bin mise system::packages::brew::lifecycle
rtk cargo test --bin mise system::packages::brew
rtk mise run test:e2e e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow
rtk mise run lint
```

The canonical oracle may remain red for missing finalization/repair until plans
011–012. Its completion marker from plan 009 must still prove execution.

## Done criteria

- One typed value is both validated and executed.
- Every formula that may mutate is prepared before any formula mutates.
- Already-current unsupported formulae do not block unrelated convergence.
- `run` has proved confinement or fails closed.
- Unsupported metadata cannot become partially installed state.

## Stop conditions

Do not implement a permissive catch-all step. Do not expand sandbox writes to
the whole home directory or filesystem. If an authoritative current formula
needs unsupported semantics, preserve fail-closed behavior and record the exact
missing lifecycle type for a follow-up implementation.
