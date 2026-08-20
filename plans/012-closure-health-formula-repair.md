# Plan 012: Add closure-aware formula health and lifecycle-only repair

Status: IN PROGRESS
Priority: P0
Effort: L
Planned against: #11910 `05ccd7ab8`, #11915 `b94b6b1c1`
Depends on: 010, 011
Implementation start: #11915 `94c66938ca1163d26de49902575f0b779367ee41`
Implementation commits: `94c66938c`, `06a9bf68c`, `57319487b`, `b5b4810c3`,
`ca2438834`, `09bccf0a7`, `56ef7caf1`, `75bdcadbd`, `c9390008a`,
`38b25fa5b`, `93df3357c`, `7c5374c0a`

Final repair-authority audit (2026-08-20): external lifecycle state was keyed
only by rack/version and could survive a real-Homebrew replacement at the same
path. Commit `ca2438834` binds every lifecycle state and repair journal to a
per-install incarnation marker, the formula snapshot digest, and a canonical
immutable native-receipt projection containing bottle/source/version/tap
identity. A different tap or receipt identity invalidates stale authority
before mutation. Legitimate Homebrew Tab rewrites remain compatible because
mutable `installed_on_request` / `installed_as_dependency`, Homebrew version,
unknown fields, and JSON key order are excluded. The same commit records exact
shared source-to-target mappings, including `.default`, so missing shared state
is repairable only from its original keg source. Regressions prove stale
different-tap state cannot execute its journal and a deleted default is restored
while user configuration plus keg/receipt/public-link inodes remain unchanged.
Historical local proof passed all 314 brew module tests; exact run
[32285796215](https://github.com/jdx/mise/actions/runs/32285796215) then passed
all 13 jobs and both authenticated oracles on
`e5fc0f5f7d1791a2ee0b1b157e1c3c6eed15fd80`. The later authority audit kept
this evidence historical rather than calling it final closure.

Final correction `56ef7caf16a5e660ce62e016acf65fa001f1433d`
binds repair, permission, and removal authority to the exact install plus full
prefix/Cellar/rack/keg/`.brew` and private-state directory identities. Prepared
removal tokens bind state hashes, canonical receipt identity, and exact
symlink type/device/inode/raw-target evidence; native same-version replacement
can discard only stale private state, never stale lifecycle effects. Typed
copy/tree/stdout/symlink effects stage atomically and use no-clobber or
revalidated overwrite authority. Every metadata error other than `NotFound`
fails closed.

Signed merge `75bdcadbdec7e2b93d50409e4f3d4fa9e319e655` integrates canonical
`origin/main` `b0795afa7d23de9eed01b3f82cde5789830fc550` without conflict. Exact
[run 32298940105](https://github.com/jdx/mise/actions/runs/32298940105) exposed
that a declared lifecycle Run target could not create its absent exact parent
under the macOS sandbox; no lifecycle marker was emitted. Linux formula proof
passed, while source proof failed closed because mandatory Landlock was
unavailable inside the disposable container.

Correction `c9390008a07ca71797b93c158e6ef5a9bff0e0ae` creates only the
declared target's missing real parent chain, binds and revalidates every
directory identity before and after the command, and removes only unchanged
empty directories on failure. The sandbox still grants no parent or sibling
subtree writes. Its macOS regression writes the declared CA-style target while
an attempted sibling overwrite remains denied. Local proof passes 392 brew
tests, 3,525 workspace/all-feature tests with four ignored, native-Linux
sandbox/cmd suites `22`/`27`, full workspace/all-feature/all-target Clippy with
`-D warnings`, formatting, and diff gates without lint exclusions. Exact hosted
[run 32302254890](https://github.com/jdx/mise/actions/runs/32302254890) on that
head failed and is retained as negative evidence. The real macOS
`ca-certificates` helper needed an adjacent temporary directory that the exact
leaf sandbox correctly denied, so the lifecycle marker was absent. The Linux
source guard also rejected the installed Homebrew runtime against its
`not-installed` declaration; unit, Windows unused-code, and aggregate gates did
not pass.

Security correction `38b25fa5b71da407a82ae12600896f2f85f1ed96`
replaces parent-subtree access with private mirrored Run outputs and
identity-bound, multi-output transactional publication. Rollback authority is
retained through validation and durability; typed published regular-file
effects record identity, mode, and content semantics for health. Missing or
changed effects are therefore classified from durable typed evidence rather
than bare required paths. Strict Linux formula execution is mandatory, while
unsupported execution fails before lifecycle mutation. Local exact-lineage
proof passes 87 lifecycle tests, all 423 brew tests, strict
workspace/all-feature/all-target Clippy, formatting, and diff checks.

Oracle-provenance correction `93df3357c6be57fae0ceda4aa03973ad8d2c407e`
splits the Linux formula and source proofs, pins executor provenance, strips CI
credentials, and makes exact job-context and marker validation part of the
aggregate. Signed merge `7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358`
integrates canonical `origin/main`
`60c5ff113a672269c1bd9455f4eb50a079371a17`; its docs, registry, and Aqua
changes do not overlap lifecycle health or repair.

Exact-head [run 32312879235](https://github.com/jdx/mise/actions/runs/32312879235)
for `7c5374c0a5d41343a4f3dfcf7e3c3e69373f6358` failed and remains negative
evidence. Linux formula completed; macOS lifecycle rejected mutable API/bottle
snapshot drift; Linux source rejected GNU `install` metadata mutation; nightly
portability and Linux Clippy gates also failed. The aggregate correctly failed.
Restore `DONE` only after a later exact head's complete workflow, authenticated
formula/source markers, and aggregate gate pass.

Acceptance extension (2026-08-16): `python@3.14` makes lifecycle permissions an
operational output. Mise-owned lifecycle state must record each affected path,
status must detect a removed owner-write bit offline, and apply may restore only
the idempotent recorded `u+w` effect. Missing or symlink-escaping targets remain
reinstall-required. Exact-head macOS proof must exercise damage and repair.

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

The downstream operator profile declares only `brew:kimi-code`. Mise poured its transitive
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

## Root-only downstream regression

The test configuration contains only:

```toml
[bootstrap.packages]
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

Completed proof:

- Root-only canonical fixture declares only `"brew:kimi-code"`. Exact-head
  [macOS job 94278577725](https://github.com/jdx/mise/actions/runs/31645663032/job/94278577725)
  installed the dependency closure, produced readable CA/OpenSSL shared state,
  passed direct OpenSSL verification and default Node fetch with no CA/TLS
  override, then detected and repaired a removed OpenSSL CA link through root
  status.
- Exact #11915 head `b5b4810c3b6420e65a277f1d5fa26adfe1b5069c`
  closed the remaining native-state gap: real-Homebrew keg-only formulae are
  classified offline from their installed formula snapshot, so an already
  current `postgresql@17` stays outside the mutation set despite unsupported
  lifecycle operations.
- [Exact-head macOS job 94429640485](https://github.com/jdx/mise/actions/runs/31694663380/job/94429640485)
  passed the production `mise bootstrap --yes` path with only the Kimi root,
  produced the required CA/OpenSSL shared state, passed direct OpenSSL and
  default Node TLS, then diagnosed and repaired a removed OpenSSL CA link.
- Its completion marker records test
  `test_system_install_brew_formula_lifecycle_macos_slow`, fixture count `4`,
  prefix `/opt/homebrew`, mise SHA `b5b4810c3b6420e65a277f1d5fa26adfe1b5069c`,
  and matching Homebrew reference/runtime `6.0.17` at
  `4dacfe77a24dead72de749c0876028b77b99cd04`.
- [Exact-head Linux/source job 94429640493](https://github.com/jdx/mise/actions/runs/31694663380/job/94429640493)
  passed the Linux formula and source-build gates. Its two markers each record
  fixture count `1`, mise head `b5b4810c3b6420e65a277f1d5fa26adfe1b5069c`,
  Homebrew reference `6.0.17` at the pinned SHA, and an intentional runtime
  value of `not-installed`.
- Full canonical legacy damage, ambiguous-source refusal, Kimi login TLS, and
  both ownership directions are descendant combined-stack Plan 018 gates; they
  are not attributed to the prerequisite-only job above.
- Local all-brew tests (207 passed) cover offline installed-receipt closure
  traversal, exact dependency/phase diagnostics, legacy lifecycle repair,
  unprovable-state reinstall classification, preserved valid topology/inodes,
  and ordered effect health.
- Current-main #11915 head `7009878b784c4ee3436d365efd2693fb4c909e50`
  passed the full [test workflow](https://github.com/jdx/mise/actions/runs/31735998370),
  including [macOS root-only lifecycle proof](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757793),
  [Linux/source proof](https://github.com/jdx/mise/actions/runs/31735998370/job/94567757706),
  macOS/Linux/Windows unit and e2e, lint, build, and the aggregate gate. The
  canonical marker records fixture count `4`, exact mise head, prefix
  `/opt/homebrew`, and matching Homebrew reference/runtime `6.0.17` at
  `4dacfe77a24dead72de749c0876028b77b99cd04`.
- Final prerequisite head `55d10319b26a27ac84477109c7ebf6fa0470af9f`
  passed [run 31915639703](https://github.com/jdx/mise/actions/runs/31915639703).
  Its macOS marker records five positive fixtures and proves the root-only
  `brew:kimi-code` dependency closure, shared OpenSSL/CA state, default Node
  TLS, legacy lifecycle diagnosis/repair, and ambiguous-state refusal against
  exact Homebrew `6.0.17` source
  `4dacfe77a24dead72de749c0876028b77b99cd04`.
- The permission-health implementation is `4a58b73b7`; harness corrections are
  `2461c2305` and `55d10319b`. Descendant combined head
  `7048be7c5b0f5bc62dc061cf32afd72a0dde9b61` directly exercises all 31
  downstream formula roots in
  [run 31921306723](https://github.com/jdx/mise/actions/runs/31921306723).
- Latest-main closure proof: merge commit
  `b112975e0d6858c2b872259970f84b4002bc9d5e` integrated `origin/main`
  `619854b468dd3fffe0d475a08d69e4c82da80acd`. Exact implementation head
  `1c3ce7cecb049a198fb64a658b9389cdbe9241d6` passed
  [run 32275082730](https://github.com/jdx/mise/actions/runs/32275082730).
  Its [macOS oracle](https://github.com/jdx/mise/actions/runs/32275082730/job/96140855866)
  validates the five-fixture lifecycle marker, including root-only Kimi
  OpenSSL/CA diagnosis and repair. Its
  [Linux/source oracle](https://github.com/jdx/mise/actions/runs/32275082730/job/96140855969)
  validates both isolated fixtures. All markers bind the exact head and pinned
  Homebrew `6.0.17` source `4dacfe77a24dead72de749c0876028b77b99cd04`.

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
