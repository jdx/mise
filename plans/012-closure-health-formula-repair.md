# Plan 012: Add closure-aware formula health and lifecycle-only repair

Status: IN PROGRESS
Priority: P0
Effort: L
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: 010, 011
Implementation start: #11915 `94c66938ca1163d26de49902575f0b779367ee41`

Drift check (2026-08-13): installed status still inspected only the configured
formula's active records. Existing lifecycle state could classify local damage,
but did not traverse installed `runtime_dependencies`, distinguish safe repair
from mandatory reinstall, or journal repair effects. The macOS lifecycle oracle
also declared the dependency formulae directly, so it could not prove the
production `brew:kimi-code` root-only configuration.

The proposed `[tools]` fixture does not invoke this subsystem in the audited
tree: native brew is a `SystemPackageManager` exposed through
`[bootstrap.packages]`, while `[tools]` accepts tool backends and has no `brew`
backend. Adding that public config surface is explicitly out of scope. The
executable regression therefore declares only `"brew:kimi-code"` under
`[bootstrap.packages]` and invokes the same `mise bootstrap --yes` entry point;
the closure and failure mode are otherwise identical.

## Objective

Make a configured brew root healthy only when its required dependency closure
is operationally healthy. Repair provable shared lifecycle damage without
repouring a valid keg or overwriting user configuration.

## Production regression

`essential-mac` declares only `brew:kimi-code`. Mise poured its transitive
`ca-certificates` and `openssl@3` dependencies with receipts marked `(mise)`,
but omitted `/opt/homebrew/etc/openssl@3` and post-install CA state. Node uses
OpenSSL's default CA store; bundled-CA mode succeeds while default TLS and Kimi
login fail. Root-only status can currently return Noop because dependency
health is not evaluated.

## Files in scope

- `src/system/packages/brew/mod.rs`
- `src/system/packages/brew/pour.rs`
- `src/system/packages/brew/lifecycle.rs`
- `src/system/packages/brew/receipt.rs` or a narrowly scoped internal state
  module
- package planning/status integration under `src/system/resources.rs` only if
  the manager contract cannot carry closure reasons
- unit/e2e fixtures

## State model

For each installed formula, distinguish:

- immutable keg and receipt identity;
- package defaults and their content/type identity;
- user-modified persistent `etc`/`var` state;
- generated post-install outputs and their health predicate;
- obsolete version-owned effects;
- operation phase and committed effects.

Mise-private state may supplement native Homebrew metadata, but absence of mise
state cannot make real-Homebrew-owned state invalid by fiat. Reconstruct health
from authoritative formula metadata, native receipts, filesystem topology, and
content provenance. Unknown ownership is `NeedsRepair` with an exact reason,
not permission to mutate.

## Implementation steps

1. Resolve the dependency closure for each configured root using normal backend
   resolution. Do not sort opaque versions locally.
2. Add a read-only health classifier per closure node: Healthy,
   LifecycleRepairable, ReinstallRequired, Unsupported, or Missing, each with
   formula/version and precise reasons.
3. Health includes keg/receipt, opt, linked-keg/public links where applicable,
   required shared `etc`/`var`, generated post-install state, and non-dangling
   required symlinks. A textual target match is insufficient if it dangles.
4. Aggregate unhealthy dependency reasons into configured-root status. Status
   performs no directory creation, network mutation, metadata write, relink, or
   command execution. It must remain deterministic offline: use installed native
   receipts/formula snapshots and already trusted cache. If required metadata is
   absent, report Unknown/ReinstallRequired; do not fetch during status or infer
   that missing metadata means no lifecycle.
5. Add a lifecycle-only apply path. It reuses plan 010 preparation and plan 011
   provenance, preserves keg/receipt/public link inodes, and touches only effects
   proven missing or formula-owned.
6. Journal every repair effect and its rollback boundary before execution.
   Resume/rollback by phase after interruption. Never blindly replay a command
   whose completion is unknown.
7. If old default content, post-install source, or ownership cannot be proven,
   return an exact reinstall requirement. Never guess from current host state.
8. Make legacy mise state recognizable: valid keg+opt, missing linked-keg,
   `.bottle/etc` retained, shared `etc` absent, post-install skipped. Report each
   missing phase separately. Repair safe phases; escalate only unprovable ones.
9. Ensure real brew can upgrade/uninstall repaired mise state and mise can adopt
   real-brew state without rewriting it merely to add private provenance.

## Essential-mac regression

The test configuration contains only:

```toml
[tools]
"brew:kimi-code" = "latest"
```

It must exercise the resolved Node → OpenSSL → CA dependency chain. Never list
those dependencies as roots. Verify:

```bash
test -r /opt/homebrew/etc/openssl@3/openssl.cnf
test -r /opt/homebrew/etc/openssl@3/cert.pem
/opt/homebrew/opt/openssl@3/bin/openssl s_client \
  -connect auth.kimi.com:443 -servername auth.kimi.com </dev/null
/opt/homebrew/opt/node/bin/node -e '
  fetch("https://auth.kimi.com/api/oauth/device_authorization").then(r => {
    if (r.status !== 405) process.exit(1)
  })'
```

The environment must not contain `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`,
`NODE_OPTIONS`, or any TLS-verification disable. Network response body is not
asserted; only verified TLS and the expected method response are relevant.

## Required tests

- Root healthy/all dependencies healthy => Noop, no writes.
- Root keg healthy/dependency shared state missing => root NeedsRepair names the
  dependency and exact phase.
- Lifecycle-only repair preserves valid keg, receipt, opt and linked-keg inodes.
- User-modified `etc` file survives repair/upgrade byte-for-byte.
- Missing provenance yields ReinstallRequired and zero mutation.
- Interrupted repair resumes or rolls back without duplicating effects.
- Legacy state described above, including missing linked-keg.
- Root-only Kimi canonical-prefix test and a hermetic local metadata fixture.

## Verification

```bash
rtk cargo test --bin mise system::packages::brew
rtk mise run test:e2e e2e/cli/test_system_install_brew_formula_lifecycle_macos_slow
rtk mise run lint
```

## Done criteria

- Configured-root status reflects required dependency health and stays read-only.
- Provable lifecycle damage repairs without repour.
- Unprovable shared state cannot be mutated.
- The root-only Kimi test passes default OpenSSL and Node trust at canonical
  prefix with no environment workaround.

## Stop conditions

Do not encode `ca-certificates`, `openssl@3`, Node, or Kimi package-specific
behavior in production. Do not claim current operator-machine repair; the
immediate `brew reinstall ca-certificates openssl@3` remains an explicit manual
operator action outside this implementation plan.
