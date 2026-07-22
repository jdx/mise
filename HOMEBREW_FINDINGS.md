# brew-cask ownership, Homebrew metadata, and canonical direction

**Research date:** 2026-07-22  
**Verification audit:** 2026-07-23 — every upstream claim re-verified against
live GitHub data and `origin/main`; see [Verification audit](#verification-audit--2026-07-23)  
**Historical direction lock (superseded by eighth pass):** 2026-07-23 —
formula-style cask identity was the **potential** product direction; see
[Upstream rejection status](#upstream-rejection-status--generate-metadata) and
[Directions that resolve the product goals](#directions-that-resolve-the-product-goals)  
**Deep research (fourth pass):** 2026-07-23 — multi-agent concept matrix A–R +
live Homebrew/Caskroom re-verify; Direction **A** (goal set) reconfirmed; empty-tab
_mechanism_ later refuted (fifth); see
[Deep research pass](#deep-research-pass--2026-07-23-fourth-pass)  
**Adversarial audit + source trace (fifth pass):** 2026-07-23 — Homebrew
uninstall/upgrade/loader traced line-by-line; **Direction A's empty-tab
mechanism refuted; intermediate Direction A2 (exact pour-time snapshot)**; see
[Fifth pass](#fifth-pass--2026-07-23-adversarial-audit-and-homebrew-source-trace)
**Empirical A2 verification + cold plan review (sixth pass):** 2026-07-23 —
sandboxed execution of brew's migration/loader on candidate metadata; A2's
_caskfile-as-authority_ variant demoted; **final mechanism = A3: brew-native
minimal installed JSON + exact projected tab** (per `plans/002`); `.rb`
caskfiles verified viable for flight-block casks; see
[Sixth pass](#sixth-pass--2026-07-23-empirical-a2-verification-and-final-mechanism)
**Seventh pass (deep research reconfirm):** 2026-07-23 — multi-agent concept
matrix + live Homebrew `@78430a54` re-verify + adversarial A3 audit; **A3
reconfirmed**; executive banner fixed A2→A3; execution order aligned to
`plans/README`; see
[Seventh pass](#seventh-pass--2026-07-23-deep-research-reconfirm-and-direction-lock)
**Eighth pass (architecture correction):** 2026-07-23 — current remote Homebrew
`@33c3da5f`, supported `brew install --cask --adopt`, cross-manager locking,
source-derived path safety, completed-action journaling, and all plans
re-audited. **A3 demoted from “locked product mechanism” to private-format
experiment.** Safe default is mise-only; best current interop candidate is an
explicit one-way Homebrew adoption/handoff; durable zero-subprocess interop
requires an upstream registration/CAS contract. See
[Eighth pass](#eighth-pass--2026-07-23-architecture-correction-and-supported-handoff-discovery).
**Process:** always extend this file when research, direction, or branch
behavior changes — it is the durable decision record for the fork.  
**Repository:** `donbeave/mise`  
**Branch:** `fix/brew-cask-homebrew-metadata-receipt`  
**Scope:** research and decision record; never an upstream PR authorization

## Executive decision

> **Current normative verdict (eighth pass): do not ship synthetic Homebrew
> metadata by default.** Homebrew has no identity-only cask marker; recognition
> grants lifecycle authority. Exactly one manager must have mutation authority.
> Keep normal direct pours mise-owned and Homebrew-invisible. First test
> Homebrew's supported `brew install --cask --adopt` as an explicit one-way
> handoff; after success Homebrew alone upgrades/uninstalls while mise observes
> and preserves. If strict no-`brew` interop remains mandatory, the durable
> solution is an upstream registration/status/deregister/handoff contract with
> Homebrew-owned validation and compare-and-swap. **A3** — minimal installed JSON
> plus a non-empty tab derived from proven completed actions — remains the best
> private serialization candidate, but only a default-off binary experiment and
> one-way handoff fallback. It is not a solved coexistence contract.

**Normative-reading rule:** this executive decision and the eighth-pass section
supersede all earlier recommendations. Earlier passes remain evidence/history,
not implementation instructions. In particular, ignore historical directions
to keep empty `uninstall_artifacts`, backfill from a live API, ship A2/A3, or let
both managers mutate the same cask.

**Current product direction (bootstrap `brew-cask:` only): explicit
single-owner lifecycle with optional handoff.** Formula-style dual-manager
coexistence is unavailable without a mutually supported coordination contract.

The original expectation was that a direct `brew-cask:` pour under Homebrew's
prefix should be **observably a Homebrew cask** without invoking `brew`. The
research below proves that cask visibility is also destructive lifecycle
authority, unlike a harmless identity tag. That expectation is therefore safe
only through explicit ownership transfer or a future supported contract.

| Layer                        | Contract                                                                                                           |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| Mise-owned pour              | mise Rust direct install; no Homebrew marker or Homebrew lifecycle promise                                         |
| Homebrew-owned install       | explicit user-selected native Homebrew install                                                                     |
| Mise ledger                  | `.mise-cask.toml` — status / paths / mise lifecycle while mise owns the cask                                       |
| Homebrew handoff             | Prefer supported `brew install --cask --adopt`; Homebrew authors its ledger and becomes sole lifecycle owner       |
| Private experiment           | A3 metadata only for disposable binary research; publishing its marker transfers mutation authority away from mise |
| Preserve                     | Never destroy genuine Homebrew-authored `.metadata` (#11012; ownership classifier — `plans/001`)                   |
| Durable zero-subprocess path | Upstream validate/register/status/deregister/handoff API with locking/CAS (`plans/009`)                            |
| Not in scope                 | Full brew replacement; identity-only markers; uncoordinated dual-writer upgrades                                   |

**Do not ship branch HEAD's empty-tab writer.** Do not mark a healthy mise pour
`Missing` because Homebrew metadata is absent. Do not automatically delete or
repair legacy synthetic trees; quarantine/recovery must be explicit. Before any
interop work, fix source-derived path containment and build a durable
completed-action manifest for cask installs.

Execution starts with [Plan 010: retire unsafe metadata](plans/010-retire-unsafe-cask-metadata.md),
[Plan 011: secure path boundaries](plans/011-secure-cask-path-boundaries.md),
[Plan 012: prove native handoff](plans/012-evaluate-homebrew-handoff.md), and
[Plan 013: record completed actions](plans/013-record-completed-cask-actions.md).
The corrected dependency/status index is [plans/README.md](plans/README.md).

**Upstream status:** Homebrew already supports full install/adoption, but no
receipt-only external registration contract was found. Neither jdx/mise nor
Homebrew has accepted the proposed zero-subprocess contract; no upstream
contact is authorized by this research.

## Repository and operating policy

This work is restricted to the fork:

| Item        | Value                                     |
| ----------- | ----------------------------------------- |
| Fork        | <https://github.com/donbeave/mise>        |
| Local clone | `/Users/donbeave/Projects/donbeave/mise`  |
| Branch      | `fix/brew-cask-homebrew-metadata-receipt` |
| Upstream    | `jdx/mise`, read-only context             |
| Fork remote | `fork`                                    |

Hard rules:

- Never open a PR, issue, discussion, or comment against `jdx/mise` from this
  work unless policy is explicitly changed.
- Push only to `donbeave/mise`.
- Public reproduction must use a minimal `mise.toml`; it must not require the
  private/reference `essential-mac` repository.
- `essential-mac` and Codex may be mentioned only as discovery context.
- Use `rtk` for shell commands.
- Mise is a binary crate for these tests; use `cargo test <filter>`, not
  `cargo test --lib`.
- Commits require `git commit -s` and:
  `Co-authored-by: Codex <codex@openai.com>`.

## User-visible problem

Given:

```toml
[bootstrap.packages]
"brew-cask:codex" = "latest"
```

mise directly downloads and installs the cask without calling `brew`. Before
this branch it creates, among other paths:

```text
/opt/homebrew/bin/codex
/opt/homebrew/Caskroom/codex/<version>/.mise-cask.toml
```

It does not create:

```text
/opt/homebrew/Caskroom/codex/.metadata/
  INSTALL_RECEIPT.json
  <version>/<timestamp>/Casks/codex.json
```

The binary works, but Homebrew reports:

```text
Error: Cask 'codex' is not installed.
```

Codex exposed this because it inferred Homebrew ownership from its path and
ran `brew upgrade --cask codex` during startup. The generic reproduction does
not require Codex:

```sh
mise bootstrap packages apply --yes
brew list --cask --versions codex
brew upgrade --cask codex
```

Bare `brew list --cask` is not a reliable check. It can print Caskroom
directory debris. `brew list --cask --versions TOKEN` exercises Homebrew's
installed metadata path.

## Historical architecture

### Formula bootstrap: direct pour with deliberate coexistence

[mise PR #10326](https://github.com/jdx/mise/pull/10326) introduced
declarative system packages and a built-in Homebrew formula installer.

Its explicit design:

- no Homebrew installation required;
- never shell out to `brew` for formula pours;
- install at Homebrew's canonical prefix because bottle paths require it;
- write brew-compatible `INSTALL_RECEIPT.json` files;
- let real Homebrew list, upgrade, and uninstall mise-poured formulae.

Formula coexistence was therefore an intentional contract, not an incidental
filesystem detail. Verified verbatim from the merged PR body (2026-07-23):

> Brew-compatible `INSTALL_RECEIPT.json` written into each keg, so a real
> Homebrew sees mise's kegs as its own — `brew list/upgrade/uninstall` all
> work — and mise's status checks count brew-installed formulae as installed.

The same body limits scope explicitly: "formulae only — no taps (require
Ruby), no casks, no services, no Intel macs, no source builds." The current
`origin/main` code carries the same contract in
`src/system/packages/brew/pour.rs`:

> brew-compatible INSTALL_RECEIPT.json so a later-installed real Homebrew
> adopts these kegs (brew list/upgrade/uninstall all work).

### Cask bootstrap: direct installer with a mise-owned ledger

[mise PR #10383](https://github.com/jdx/mise/pull/10383) added direct taps and
casks. It replaced an abandoned direction that proxied unsupported behavior to
the Homebrew CLI.

Its explicit design:

- fetch API metadata directly;
- download and verify artifacts directly;
- install supported artifact types directly;
- record local state in `.mise-cask.toml`;
- fail unsupported artifacts explicitly;
- do not fall back to `brew install --cask`.

Unlike formula support, this PR did not promise that Homebrew would adopt
mise-originated casks. Verified 2026-07-23: the merged body describes only a
manager that "downloads cask artifacts directly, verifies checksums, installs
supported app bundles, and reports status from local Caskroom/receipt state";
its diff introduced the mise-owned `.mise-cask.toml` receipt and contains zero
occurrences of `INSTALL_RECEIPT`.

### Homebrew-origin metadata preservation

[Discussion #11007](https://github.com/jdx/mise/discussions/11007) reported
that mise deleted `.metadata` from casks originally installed by Homebrew.

Root cause:

1. Mise counted `.metadata` as another Caskroom version.
2. It entered the reinstall path.
3. Stale-version cleanup deleted `.metadata`.
4. Homebrew could no longer inspect or upgrade its own cask.

[mise PR #11012](https://github.com/jdx/mise/pull/11012) fixed this by ignoring
`.metadata` during version detection and preserving it during cleanup.

jdx's response is important:

> For casks already affected, the deleted metadata cannot be recovered by
> mise; `brew reinstall --cask --force <cask...>` will recreate it.

Source: [jdx discussion comment](https://github.com/jdx/mise/discussions/11007#discussioncomment-17651553).

That work establishes **preservation of another manager's ownership**, not
automatic reverse adoption.

Two nuances confirmed on 2026-07-23:

- The discussion body (by the reporter, khoi) explicitly proposed "generate
  Homebrew-compatible cask metadata" as one of two expected behaviors. The
  merged fix chose ignore-and-preserve only; jdx's comment is silent on the
  generate option. That is a revealed preference for preserve-only, not an
  explicit rejection of metadata generation.
- The "cannot be recovered by mise" sentence describes mise's then-current
  capability. It is not a prohibition on mise ever writing cask metadata.

## What the metadata branch implemented

The branch commits before this decision record were:

| Commit      | Purpose                                            |
| ----------- | -------------------------------------------------- |
| `bd2fe92bd` | Write Homebrew `.metadata` after a mise cask pour  |
| `300c5c062` | Record initial root-cause research                 |
| `2712729ba` | Remove dangerous partial uninstall metadata        |
| `99e2e50a5` | Backfill earlier mise-only pours through bootstrap |

The implementation writes:

```text
Caskroom/<token>/.metadata/
  INSTALL_RECEIPT.json
  config.json
  <version>/<UTC timestamp>/Casks/<token>.json
```

Important choices:

- installed cask JSON is `{}`;
- timestamp matches `%Y%m%d%H%M%S.%L` in UTC;
- tab uses `homebrew_version: "5.1.15 (mise)"`;
- tab has `uninstall_artifacts: []`;
- mise's `.mise-cask.toml` remains present;
- existing Homebrew metadata is preserved;
- unrelated Caskroom debris is not adopted;
- backfill requires a matching `.mise-cask.toml`;
- installed caskfile is written last as a validity marker.

### Why partial uninstall metadata was removed

An early implementation reconstructed a non-empty subset of app/binary/pkg
artifacts. That was unsafe.

Homebrew's `CaskLoader.resolve_installed_artifacts` treats any non-empty tab
list as authoritative and returns without API recovery. A partial list can
therefore suppress cleanup of artifacts omitted by mise.

The hardened branch writes an empty list so an online Homebrew can recover the
current definition from its API. This is safer than a lying partial list, but
it remains fallback-grade rather than exact historical metadata.

### Backfill call-graph correction

The first backfill implementation repaired metadata inside `install_one`'s
“already installed” path. That path was unreachable during normal apply:

1. The package driver calls `manager.installed()`.
2. It removes `PackageState::Installed` requests.
3. `manager.install()` never receives them.

The branch was corrected so a mise-owned cask without a Homebrew installed
caskfile reports out-of-sync, allowing explicit apply to select a no-download
repair. Status remains read-only and dry-run reports the planned repair.

This made backfill operational, but it also exposed a product problem: a
working mise-owned cask is now called “missing” solely because a different
manager's private ledger is absent.

## Homebrew implementation findings

The implementation was audited on Homebrew 6.0.12+ at commit
[`78430a54`](https://github.com/Homebrew/brew/tree/78430a54dd972a9725cf5f9a862bacd330303906).

### Installed predicate

[`Cask#installed?`](https://github.com/Homebrew/brew/blob/78430a54dd972a9725cf5f9a862bacd330303906/Library/Homebrew/cask/cask.rb)
is effectively:

```ruby
installed_caskfile&.exist? || false
```

[`Caskroom.cask_installed_caskfile`](https://github.com/Homebrew/brew/blob/78430a54dd972a9725cf5f9a862bacd330303906/Library/Homebrew/cask/caskroom.rb)
selects the latest path under:

```text
.metadata/*/*/Casks/<token>.{json,internal.json,rb}
```

The caskfile path, not the payload directory or `.mise-cask.toml`, controls
Homebrew's installed gate.

### Timestamp and tab

Homebrew uses `%Y%m%d%H%M%S.%L` and UTC. Its cask tab lives at
`.metadata/INSTALL_RECEIPT.json` and records source version, tap, architecture,
time, runtime dependencies, and uninstall-relevant artifacts.

### Installed JSON

Current Homebrew intentionally keeps installed JSON minimal. It stores
post-install data needed for future uninstall/reinstall/upgrade and relies on
the receipt for exact installed version and uninstallable artifacts.

See the official internal design document:
[JSON API post-install metadata plan](https://docs.brew.sh/rubydoc/file.json_api_postinstall_preflight_postflight_plan.html).

### Private API boundary

Homebrew labels the relevant Ruby cask tab and uninstall APIs private. Third
parties are warned that they can change without notice:

- [Cask::Tab](https://docs.brew.sh/rubydoc/Cask/Tab)
- [Cask::Artifact::Uninstall](https://docs.brew.sh/rubydoc/Cask/Artifact/Uninstall.html)
- [Cask::Artifact::AbstractUninstall](https://docs.brew.sh/rubydoc/Cask/Artifact/AbstractUninstall.html)

Formula receipt compatibility is also implementation coupling, but jdx made
it an explicit formula design requirement. No equivalent explicit decision
has been found for mise-originated casks.

## Live verification

### Initial machine state

Read-only checks found:

| Token           | Result before repair                                                  |
| --------------- | --------------------------------------------------------------------- |
| `kimi`          | Homebrew recognized it after an earlier synthetic metadata experiment |
| `codex`         | Homebrew recognized it after real Homebrew adoption                   |
| `grok-build`    | `Cask 'grok-build' is not installed`                                  |
| `codexbar`      | not installed according to Homebrew                                   |
| `claude-code`   | not installed according to Homebrew                                   |
| `1password-cli` | not installed according to Homebrew                                   |

The failing entries contained usable mise payloads and `.mise-cask.toml` but
no Homebrew installed caskfile.

### Branch E2E using `grok-build`

The branch binary was built and run against the existing mise-owned
`grok-build` cask.

Dry run:

```text
repair cask metadata grok-build/0.2.106
mise brew-cask:grok-build: already installed
```

Apply:

```text
mise brew-cask:grok-build: repaired missing Homebrew metadata
mise brew-cask:grok-build: already installed
```

Homebrew then reported:

```text
grok-build 0.2.106
Warning: Not upgrading grok-build, the latest version is already installed
```

This proves:

- bootstrap selected the repair;
- no cask artifact download was needed;
- the metadata path satisfies Homebrew's installed gate;
- list/info/upgrade dry-run operate.

It does **not** prove:

- complete uninstall;
- rollback after failed upgrade;
- exact offline recovery;
- pkg cleanup;
- app-layout parity;
- hook parity;
- renamed-token migration;
- `version :latest` behavior;
- compatibility with future Homebrew metadata changes.

During the online check, Homebrew recovered `grok-build` artifacts from its API
and expanded the initially empty installed JSON. This confirms the fallback
works online. It also demonstrates that Homebrew may infer artifacts that mise
did not install.

### Tests run

Focused and module tests passed:

```text
cargo test repair_homebrew_cask_metadata       # earlier helper version
cargo test write_homebrew_cask_metadata
cargo test homebrew_cask_receipt
cargo test format_homebrew_timestamp
cargo test system::packages::brew::cask        # 63 passed
```

Build completed with existing/non-blocking warnings. The experiment proved
mechanical compatibility, not the final ownership decision.

## Official mise contract

Current official pages:

- [Bootstrap](https://mise.jdx.dev/bootstrap.html)
- [Bootstrap packages](https://mise.jdx.dev/bootstrap/packages/)
- [Homebrew bootstrap manager](https://mise.jdx.dev/bootstrap/packages/brew.html)

They establish:

- bootstrap is explicit, declarative machine convergence;
- packages are machine-global and separate from `[tools]`;
- `brew` and `brew-cask` use built-in installers and do not require Homebrew;
- unsupported cask behavior fails instead of delegating;
- formulae have explicit real-Homebrew coexistence;
- cask import/prune remains unavailable until uninstall semantics are safe.

The upstream docs do not promise that mise-originated casks are Homebrew-owned.
The cask coexistence text present on this fork branch was introduced by this
metadata experiment, not inherited from upstream.

## jdx's demonstrated direction

The clearest current statement is in
[discussion #10582](https://github.com/jdx/mise/discussions/10582#discussioncomment-17563994):

> a mise-owned cask lifecycle runtime path

and:

> intentionally not a `brew install --cask` fallback

Unsupported behavior should fail loudly so the implementation can expand
deliberately. Both phrases were verified verbatim on 2026-07-23.

**Attribution caveat (added 2026-07-23):** nearly every substantive cask
comment from the `jdx` account — #10582, #11007, #11058, #11168 — ends with
"_This comment was generated by an AI coding assistant._" They are posted from
jdx's account and remain directional evidence, but they are not
personally-written position statements. The only clearly human-toned jdx
message found in these threads is in
[discussion #11157](https://github.com/jdx/mise/discussions/11157):

> why all the noise? there's no need for discussions with a bunch of replies
> alongside a PR—just make a PR

That message also documents jdx's preferred channel for proposals: a small
focused PR, not a discussion.

This direction is consistent across the implementation history:

1. Reject brew CLI proxying.
2. Fetch exact API/source metadata directly.
3. Implement artifact types incrementally.
4. Track installed paths in `.mise-cask.toml`.
5. Preserve files owned by Homebrew.
6. Avoid destructive cask import/prune until safe.

Fresh jdx-authored work reinforces this (states verified 2026-07-23; both
open, both created 2026-07-22):

- [PR #11197](https://github.com/jdx/mise/pull/11197): structured flight
  steps and correct zap/uninstall receipt distinctions. Its diff reads cask
  JSON fields and pkgutil receipt IDs only; it writes no `.metadata`
  directories and no `INSTALL_RECEIPT`.
- [PR #11198](https://github.com/jdx/mise/pull/11198): declared/generated
  completions with mise-owned install, upgrade, status, and cleanup tracking.
  It extends only the mise-owned `CaskReceipt` (`.mise-cask.toml`).

This is a trajectory toward a complete direct cask manager, not a wrapper that
hands every install to Homebrew after pouring it.

## Complete open upstream inventory

Snapshot taken 2026-07-22 using GitHub REST search, the open pulls/issues
endpoints, and paginated Discussions GraphQL. Results were filtered for
`brew`, `homebrew`, `cask`, and `bootstrap packages`, then manually classified
to remove incidental mentions such as installing mise itself with Homebrew.

### Open GitHub Issues

No open GitHub Issues concern bootstrap Homebrew support. The project currently
uses Discussions for these user reports.

### Open pull requests

| PR                                               | Relevance                                                                |
| ------------------------------------------------ | ------------------------------------------------------------------------ |
| [#11197](https://github.com/jdx/mise/pull/11197) | Direct cask lifecycle and receipt correctness; architecturally important |
| [#11198](https://github.com/jdx/mise/pull/11198) | Direct completion ownership; architecturally important                   |
| [#11139](https://github.com/jdx/mise/pull/11139) | Release aggregation containing recent cask fixes                         |
| [#11172](https://github.com/jdx/mise/pull/11172) | Homebrew sync keyword match; unrelated to bootstrap cask ownership       |

### Open discussions directly relevant to bootstrap Homebrew/casks

| Discussion                                              | Subject                     | Direction/effect                                                                                    |
| ------------------------------------------------------- | --------------------------- | --------------------------------------------------------------------------------------------------- |
| [#10413](https://github.com/jdx/mise/discussions/10413) | Declarative package pruning | Preceded formula import/prune; jdx's entire reply is "sounds fine" — no cask rationale stated there |
| [#10582](https://github.com/jdx/mise/discussions/10582) | Broader cask types          | Explicit mise-owned lifecycle; fail loudly                                                          |
| [#10598](https://github.com/jdx/mise/discussions/10598) | `1password-cli` binary      | Led to direct binary support                                                                        |
| [#10625](https://github.com/jdx/mise/discussions/10625) | Claude Code raw archive     | Led to direct extraction support                                                                    |
| [#10684](https://github.com/jdx/mise/discussions/10684) | Completions/manpages        | Direct artifact coverage                                                                            |
| [#10764](https://github.com/jdx/mise/discussions/10764) | VS Code suffixless ZIP      | Direct archive sniffing                                                                             |
| [#10765](https://github.com/jdx/mise/discussions/10765) | Font target expansion       | Direct font support                                                                                 |
| [#10782](https://github.com/jdx/mise/discussions/10782) | Cask appdir options         | Unresolved configuration surface                                                                    |
| [#10917](https://github.com/jdx/mise/discussions/10917) | Localized casks/Ruby        | Direct cask DSL execution                                                                           |
| [#10968](https://github.com/jdx/mise/discussions/10968) | Intel macOS Homebrew        | jdx explicitly declined Intel support                                                               |
| [#11007](https://github.com/jdx/mise/discussions/11007) | Deleted brew metadata       | Preserve genuine Homebrew ownership                                                                 |
| [#11058](https://github.com/jdx/mise/discussions/11058) | `__MACOSX` artifact twin    | Direct artifact lookup fix                                                                          |
| [#11156](https://github.com/jdx/mise/discussions/11156) | Yaak bundle case            | Direct filesystem parity                                                                            |
| [#11157](https://github.com/jdx/mise/discussions/11157) | VLC generated wrapper       | Direct lifecycle parity                                                                             |
| [#11168](https://github.com/jdx/mise/discussions/11168) | Docker `/usr/local` targets | Direct target-path parity                                                                           |

No open upstream PR, Issue, or Discussion found in this inventory proposes
automatic Homebrew metadata creation for mise-originated casks.

## Community usage evidence

Recent independent walkthroughs treat `brew-cask:` as declarative bootstrap
state applied by mise:

- [Zenn: following mise's latest bootstrap features](https://zenn.dev/boykush/articles/8d3f52c1a97b04)
- [DevelopersIO: completing machine setup with mise bootstrap](https://dev.classmethod.jp/articles/setup-machine-with-mise-bootstrap/)

They support the product value of direct declarative cask management. They do
not establish that Homebrew should become a second automatic owner.

## Competing designs

### A. Blanket metadata emission — current branch

Advantages:

- fixes Homebrew list/info/upgrade installed gate;
- fixes Codex's immediate path-based updater failure;
- resembles formula receipt coexistence;
- does not require the brew CLI.

Problems:

- creates an ownership claim stronger than actual lifecycle parity;
- couples mise to private Homebrew implementation;
- online fallback can infer artifacts mise never installed;
- offline uninstall metadata is incomplete;
- cask import/prune is still considered unsafe upstream;
- changes mise status semantics to require another manager's ledger;
- writes Homebrew metadata even where Homebrew is absent;
- runs against the revealed preference in #11007/#11012: offered the
  "generate Homebrew-compatible cask metadata" option in the discussion body,
  upstream shipped ignore-and-preserve only.

Decision: do not ship as default.

### B. Emit only for simple binary casks

Advantages:

- Codex-class binary layouts are closer to Homebrew;
- smaller lifecycle mismatch than app/pkg casks.

Problems:

- creates surprising artifact-dependent ownership;
- completion and hook gaps still exist;
- still consumes a private Homebrew API;
- unclear behavior when a cask changes artifact type;
- no upstream design signal endorses it.

Decision: potentially experimental, not canonical default.

### C. Emit only when real Homebrew exists

Advantages:

- avoids unused metadata on brew-less machines;
- directly targets coexistence environments.

Problems:

- presence of `brew` does not authorize ownership transfer;
- later Homebrew installation leaves earlier casks ambiguous;
- lifecycle mismatch remains;
- status changes depending on another executable's presence.

Decision: insufficient.

### D. Explicit adoption/handoff

Advantages:

- user chooses dual ownership or transfers ownership;
- can be limited to supported casks;
- can run strong preflight checks;
- can document irreversible/lifecycle effects.

Problems:

- no current upstream command or contract;
- exact metadata remains private;
- safest author is Homebrew itself, which conflicts with the built-in
  installer's no-brew default if made automatic;
- needs rollback and cross-version tests.

Decision: best future interoperability shape if jdx explicitly wants it.

### E. Single owner — main status quo / ops workaround (not product finish)

For a mise-owned cask:

```text
mise config → mise direct pour → .mise-cask.toml → mise status/upgrade
```

For a Homebrew-owned cask:

```text
brew install → Homebrew .metadata → mise preserves/adopts presence
```

Advantages:

- matches **current** `origin/main` code;
- no private-API liability;
- essential-mac Codex mitigation works with `check_for_update_on_startup = false`.

Cost:

- fails product goals G2–G4 (brew identity / Codex brew updater / user expectation
  of `brew-cask:`);
- `brew doctor` noise on mise-only Caskroom state.

Decision: **ops under main only** — not the locked potential product direction.
See Direction **A** (executive + goal matrix). Also note competing-designs
section **A** earlier in this doc was the “blanket metadata” branch shape; the
locked product Direction A is **pour-time identity without status lie**, refined
in the 2026-07-23 third pass.

## Canonical resolution of the Codex case

### Mise owns Codex

```text
[bootstrap.packages]
"brew-cask:codex" = "latest"

mise bootstrap packages upgrade --manager brew-cask
```

Codex's Homebrew startup updater must be disabled because the install is not
Homebrew-owned. This is the current `essential-mac` mitigation via
`check_for_update_on_startup = false`.

### Homebrew owns Codex

Install Codex with real Homebrew. Homebrew writes its own complete current
metadata and Codex can legitimately invoke `brew upgrade --cask codex`.

### Root cause framing

The durable bug class is ambiguous ownership:

```text
canonical Homebrew-looking path
    does not imply
Homebrew-authored lifecycle state
```

The metadata branch fixes the inference by making it true enough for
`installed?`. The canonical current solution is to keep updater behavior
consistent with the actual owner.

## If Homebrew interoperability is pursued later

Minimum acceptance gate:

1. Explicit jdx product decision that mise casks may become Homebrew-owned.
2. Exact ownership semantics: coexistence, transfer, or opt-in adoption.
3. Current-version metadata captured at pour time; no guessed historical
   backfill.
4. Complete representation of artifacts actually installed by mise.
5. App, binary, font, pkg, completion, flight-step, and uninstall parity.
6. Renamed-token and tap migration behavior.
7. `version :latest`, `sha256 :no_check`, and `url_specs.only_path` coverage.
8. Transactional metadata writes and rollback.
9. Test matrix:
   - install;
   - list/info;
   - upgrade current and outdated;
   - uninstall;
   - reinstall;
   - `--zap`;
   - online and offline;
   - current and previous supported Homebrew releases.
10. Compatibility adapter isolated from core cask logic because Homebrew's API
    is private.
11. Honest docs that do not promise more than tested lifecycle parity.

Until those conditions hold, a synthetic installed caskfile is an installed
gate workaround, not a complete compatibility contract.

## Recommended branch next step

Aligned with **Direction A** (potential formula-style cask identity):

1. Keep this document as the decision record; **always extend it** on new
   research or behavior changes.
2. Keep pour-time `.metadata` write + empty `uninstall_artifacts` + no status
   `Missing` when only brew tab is absent (HEAD `a47633fc2`).
3. Keep #11012 preserve behavior for foreign brew metadata.
4. Repair already-poured mise casks on upgrade/re-invoke, not via status lie.
5. Harden tests/docs caveats; do not claim full uninstall/zap parity.
6. Rebase onto main after #11197/#11198 if preparing any future upstream-shaped
   slim PR (policy still forbids opening one unless explicitly lifted).
7. essential-mac: A unblocks brew identity; Codex flag remains optional if mise
   should own version bumps exclusively.

## Verification audit — 2026-07-23

A second, independent verification pass re-checked every upstream claim in
this document against live GitHub data (PR bodies and merge states, exhaustive
PR/issue search, discussion comments via GraphQL) and against the code and
docs on `origin/main`. Overall: the document's load-bearing claims all held.
Three corrections were required; they are folded into the sections above and
listed here.

### Claim-by-claim results

| Claim                                                                                                              | Verdict                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| #10326 promises full brew coexistence for formulae                                                                 | Verified, verbatim ("sees mise's kegs as its own")                                                                                     |
| #10326 scope excludes casks                                                                                        | Verified, verbatim ("no casks")                                                                                                        |
| #10383 makes no brew-compat promise for casks                                                                      | Verified; zero `INSTALL_RECEIPT` occurrences in its diff                                                                               |
| #11012 preserves, never creates, `.metadata`                                                                       | Verified                                                                                                                               |
| jdx #10582 quotes ("mise-owned cask lifecycle runtime path", "intentionally not a `brew install --cask` fallback") | Verified, verbatim                                                                                                                     |
| jdx #11007 quote ("cannot be recovered by mise…")                                                                  | Verified, verbatim                                                                                                                     |
| #11197/#11198 open, mise-owned receipts only                                                                       | Verified; no `.metadata` writes in either diff                                                                                         |
| No upstream PR _implements_ brew `.metadata` for mise-poured casks                                                 | Verified — zero PRs                                                                                                                    |
| No discussion body ever mentions generate as expected option                                                       | **Corrected 2026-07-23 third pass** — #11007 OP explicitly offered preserve **or generate**; no _follow-up PR_ proposed implementation |
| Upstream docs coexistence section is formulae-only                                                                 | Verified against `origin/main` `docs/bootstrap/packages/brew.md`; the cask coexistence text exists only on this branch                 |

Search-hit classification notes:

- [#11107](https://github.com/jdx/mise/pull/11107) "support auto-updating
  cask metadata" (merged 2026-07-20) concerns the cask JSON `auto_updates`
  field, not `.metadata` directories, despite the title.
- [#11164](https://github.com/jdx/mise/pull/11164) (this fork's author) and
  [#11174](https://github.com/jdx/mise/pull/11174), both merged 2026-07-21,
  are artifact-lookup/symlink fixes with no ownership change.
- The only `INSTALL_RECEIPT` work upstream is formulae-only (#10326).

### Corrections applied

1. **Attribution.** Nearly every substantive cask comment from the `jdx`
   account (#10582, #11007, #11058, #11168) is self-labeled
   "generated by an AI coding assistant". Posted from his account, still
   directional, but this document previously cited them as personally-written
   positions. The one clearly human-toned jdx message found is #11157's
   "just make a PR".
2. **#10413 overstated.** jdx's entire comment there is "sounds fine". Any
   import/prune design rationale lives in the follow-up PRs, not in a jdx
   statement in that discussion.
3. **"Contradicts jdx guidance" was an overread.** The #11007 sentence
   described capability, not policy. The accurate signal is revealed
   preference: the discussion body proposed metadata generation and the
   merged fix shipped ignore-and-preserve only.

### Per-component fit against jdx's direction

The branch is not one decision; its components fit differently:

| Branch component                                                      | Fit under Direction A (potential)                                                             |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Pour-time `.metadata` write on fresh install                          | **Core of A** — formula #10326 analogue; exact current version                                |
| Repair of mise-owned pours missing brew tab (`.mise-cask.toml` proof) | **OK for A** — not “recover deleted brew-origin metadata”; only adopt earlier mise-only pours |
| Status flip (`Missing` when brew ledger absent)                       | **Removed on HEAD** — was wrong; mise status stays on payload/mise ledger                     |
| Empty `uninstall_artifacts` tab                                       | Correct for brew API early-return; document offline/uninstall gaps                            |

The formula/cask asymmetry is therefore **undecided upstream, not decided
against**: jdx shipped exactly this coexistence contract for formulae, and no
statement for or against the cask equivalent exists anywhere upstream.

### Would this branch be accepted as an upstream PR as-is?

**As a fat research dump: unlikely.** Strip `HOMEBREW_FINDINGS.md` /
interop essays from any upstream PR; jdx process norm is a small focused PR
(#11157).

**As slim pour-time identity (Direction A): maybe / medium** — no rejection
proof; positive formula precedent (#10326); must frame honestly (installed gate,
not full lifecycle); rebase after #11197/#11198 (same files); no status
`Missing` when only brew tab absent (fixed on HEAD `a47633fc2`).

At the 2026-07-23 audit the branch was a few commits behind `origin/main`; none
of those commits implemented cask `.metadata` generation.

## Upstream rejection status — generate metadata

**Question:** Did jdx reject “mise writes Homebrew cask `.metadata` for
mise-originated pours so brew recognizes the install?”

**Answer: no. Not rejected. Not accepted either.**

### Exhaustive look (2026-07-23)

| Looked for                                                            | Result                                                                                              |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Discussion proposing generate as expected behavior                    | **#11007** (khoi) — preserve **or generate**                                                        |
| Merged response to #11007                                             | **#11012** — preserve/ignore only; does **not** generate                                            |
| jdx text “do not generate” / “won’t support brew list for mise casks” | **None**                                                                                            |
| Open or closed PR that implements cask `.metadata` generation         | **None**                                                                                            |
| Issue tracker WONTFIX for this                                        | **None** (bootstrap cask reports live in Discussions)                                               |
| Title trap                                                            | **#11107** “auto-updating cask metadata” = cask JSON `auto_updates` field, not Caskroom `.metadata` |

### #11007 / #11012 (only related thread)

Reporter expected:

> preserve existing Homebrew metadata **or generate** Homebrew-compatible cask
> metadata

jdx (AI-labeled) confirmed deletion bug; shipped #11012 preserve; recovery for
already-deleted brew metadata is `brew reinstall --cask --force`, not mise.

**Correct reading:** bug-fix scope was “stop destroying brew’s ledger.”  
**Incorrect reading:** “jdx vetoed generation forever.”  
“Cannot be recovered by mise” describes **deleted brew-origin** tabs, not a ban
on writing tabs for **new mise pours**.

### Weight for “potential direction”

- No reject proof → **may pursue Direction A**.
- No accept proof → do not claim jdx already agreed.
- Silence after #11012 is **incomplete product**, not statute.

## Product goals this work must resolve

| ID  | Goal                                                                                         |
| --- | -------------------------------------------------------------------------------------------- |
| G1  | Install via `mise bootstrap` / `brew-cask:` (Rust pour, no `brew install --cask`)            |
| G2  | User should not feel a different product than `brew install --cask` for **install identity** |
| G3  | `brew list --cask --versions TOKEN` works                                                    |
| G4  | `brew upgrade --cask TOKEN` installed-gate works (Codex-class tools)                         |
| G5  | mise still status/upgrade via bootstrap (`.mise-cask.toml`)                                  |
| G6  | Never destroy genuine Homebrew `.metadata` (#11012)                                          |
| G7  | Bootstrap-scoped — not full Homebrew replacement                                             |

**Not required for “identity done”:** perfect offline uninstall of every app
layout, full zap parity, Intel macs, every artifact type.

## Directions that resolve the product goals

| ID    | Direction                                                 | Full goal set?     | jdx risk         | Notes                                                  |
| ----- | --------------------------------------------------------- | ------------------ | ---------------- | ------------------------------------------------------ |
| **A** | Pour-time write brew `.metadata` + keep `.mise-cask.toml` | **Yes (identity)** | Med (open)       | Only complete match for G1–G7 without shelling to brew |
| **B** | Single owner + disable tool brew self-update              | No                 | Low (main today) | Ops workaround; G3/G4 stay broken                      |
| **C** | Install those casks with real `brew` only                 | Partial            | Low              | Abandons pure mise pour                                |
| **D** | Shell out to `brew install --cask`                        | No                 | **High reject**  | Explicit non-goal (#10582)                             |
| **E** | Metadata only for simple binary casks                     | Partial            | Med              | Codex-class only; inconsistent ownership               |
| **F** | Explicit opt-in adopt/handoff command                     | Partial            | Med              | Default still feels wrong                              |
| **G** | Fix only Codex / third-party tools                        | No                 | n/a              | `brew` CLI still broken for humans                     |
| **H** | zerobrew-style private store                              | No                 | n/a              | Not brew-visible; wrong reference                      |

### zerobrew (checked, not a reference for A)

zerobrew cask support (as of research): binary-only; stages into its own Cellar
name `cask:token`; private sqlite ledger; **does not** write Homebrew
`Caskroom/.../.metadata`. Parallel package manager — not “brew recognizes
install.” **Right references for A:** mise formula `pour::write_receipt` +
Homebrew `Cask#installed?` + this branch’s writer.

### Only A fully hits G1–G7 together

B is what main forces today and what essential-mac already mitigates for Codex
(`check_for_update_on_startup = false`). B does **not** finish the product
expectation that `brew-cask:` feels like brew.

## Direction A — locked potential direction

```text
mise bootstrap brew-cask:TOKEN
  → pour artifacts (Rust)
  → write .mise-cask.toml          # mise status / upgrade
  → write .metadata/…              # brew installed? gate
  → preserve pre-existing brew .metadata on cleanup (#11012)
```

**Branch HEAD behavior (after `a47633fc2`):**

- pour-time `write_homebrew_cask_metadata`
- empty `uninstall_artifacts` (online API fallback)
- repair on re-invoke of already-current install (upgrade path includes
  Installed packages)
- status does **not** use Missing when only brew tab is absent

**Honest limits (document, do not overclaim):**

- installed gate ≠ full lifecycle parity
- empty uninstall tab + app copy-vs-move → some `brew uninstall` cases differ
- dual upgrade (mise vs brew) can race if both used carelessly

**Upstream PR shape if policy ever allows:**

1. Small: `cask.rs` + short docs coexistence caveats only
2. Frame as #10326 cask extension
3. Rebase after #11197 / #11198
4. No research novel / findings file in the PR
5. Tests: layout, preserve foreign, empty uninstall_artifacts

## Final conclusion

The investigation found a real ledger mismatch and a real third-party failure
(Codex / `brew upgrade --cask`). The branch proved the minimal filesystem
condition Homebrew uses for `installed?`.

**Upstream did not reject** generating brew-compatible cask metadata for
mise pours. The only related ship was #11012 **preserve** after brew metadata
was deleted — a different bug. Formula coexistence (#10326) remains the
strongest positive precedent for writing brew-native ledgers at pour time.

**Potential direction (locked):** Direction **A** — formula-style cask identity
for bootstrap `brew-cask:` (mise pour + dual ledger: `.mise-cask.toml` +
`.metadata`). That is the only researched option that resolves G1–G7 without
shelling out to `brew` or abandoning the `brew-cask:` product name.

**Not the finish line:** single-owner-only ops (B) — valid under main today,
insufficient for user expectation of brew-identical identity.

**Proven jdx accept:** no. **Proven jdx reject:** no. Proceed as potential
fork direction; any upstream PR remains a separate, policy-gated decision.

## Independent re-audit errata — 2026-07-23 (second pass)

Re-confirmed load-bearing facts (live GitHub, `origin/main` @ `e3f5ddef2`,
Homebrew 6.0.12, subagent cross-checks).

1. **#11157 “just make a PR”** is a jdx **nested reply** (2026-07-21T14:25:17Z).
2. **Live Caskroom drift** on the audit machine: some tokens still fail
   `brew list --cask --versions` without `.metadata`; others later brew-adopted.
3. Evidence bundle in research scratch. No upstream PR/issue/comment opened.

## Direction lock extension — 2026-07-23 (third pass)

After rejection-status research and full goal/direction matrix:

1. **Generate-metadata was never rejected** — see
   [Upstream rejection status](#upstream-rejection-status--generate-metadata).
2. **Direction A is the potential product direction**; B is ops-only under main.
3. **zerobrew is not a model** for brew-visible casks.
4. **Always extend this document** when research, direction, or branch behavior
   changes (process rule for this fork track).
5. Branch code on HEAD implements A’s core (pour-time write, no status lie);
   docs under `docs/bootstrap/packages/brew.md` describe cask coexistence with
   caveats.

## Deep research pass — 2026-07-23 (fourth pass)

Multi-agent re-verification + expanded concept space. Goal: confirm or
replace Direction A as the single most reasonable product direction.

**Agents:** Homebrew source explore · mise dual-ledger explore · live
upstream GitHub verify · concept brainstorm (I–R) · live Caskroom probe.

**Scratch evidence:** implementer `verification.log`, `homebrew_source.log`,
`cargo_test_metadata.log`, `subagent_notes.md`.

### Naming disambiguation (do not merge)

| Name                                     | Meaning                                                                                       | Ship?                   |
| ---------------------------------------- | --------------------------------------------------------------------------------------------- | ----------------------- |
| Competing-design **A** (earlier section) | Early branch: pour-time metadata **plus** status `Missing` when brew tab absent               | **No** as status policy |
| Product **Direction A**                  | Pour-time dual ledger; mise status stays on payload/`.mise-cask.toml`; repair on upgrade path | **Yes (potential)**     |
| **A-status-lie**                         | Historical bug only — not a separate product concept                                          | fixed on HEAD           |

Same filesystem mechanism. Difference = **who owns mise package status**.

### Load-bearing claims re-verified (this pass)

| #   | Claim                                                                                 | Method                                                               | Verdict                                                                                                                                                 |
| --- | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `Cask#installed?` ⇔ installed caskfile exists                                         | Local Homebrew 6.0.12-92-g78430a5 `cask.rb:244–246`                  | **Hold**                                                                                                                                                |
| 2   | Path = `.metadata/*/*/Casks/<token>.{json,internal.json,rb}` (max timestamp basename) | `caskroom.rb:47–61`                                                  | **Hold**                                                                                                                                                |
| 3   | `INSTALL_RECEIPT.json` **not** required for `installed?`                              | `Cask::Tab` at `.metadata/INSTALL_RECEIPT.json`; gate never reads it | **Hold** (tab still needed for good uninstall recovery)                                                                                                 |
| 4   | Non-empty `uninstall_artifacts` early-returns; empty allows API recover               | `cask_loader.rb:841–844` `artifacts.presence`                        | **REFUTED in fifth pass** — mechanically true, conclusion wrong: `.presence` makes `[]` ≡ _missing_; fallback is version-unchecked current-API recovery |
| 5   | Formula pour writes brew-compatible keg receipt                                       | `pour.rs:181–258`                                                    | **Hold**                                                                                                                                                |
| 6   | HEAD: pour-time `write_homebrew_cask_metadata` + no status Missing on missing tab     | `cask.rs:171–188`, `241–253`                                         | **Hold**                                                                                                                                                |
| 7   | Live pure-mise pours fail brew; hybrid pass                                           | Caskroom cohort: 12 pure-mise error; `grok-build`/`kimi` OK          | **Hold**                                                                                                                                                |
| 8   | #10326 formula coexistence promise                                                    | Live PR body                                                         | **Hold**                                                                                                                                                |
| 9   | #10383 no cask INSTALL_RECEIPT design                                                 | Live PR + main `cask.rs`                                             | **Hold**                                                                                                                                                |
| 10  | #11007 OP offered generate; #11012 preserve-only; no “do not generate”                | Live discussion/PR                                                   | **Hold**                                                                                                                                                |
| 11  | No upstream PR implements cask `.metadata` generation                                 | Search (title trap #11107 = `auto_updates`)                          | **Hold**                                                                                                                                                |
| 12  | #10582 rejects `brew install --cask` fallback                                         | Live discussion                                                      | **Hold** → concept D **HIGH-REJECT**                                                                                                                    |

Unit tests on HEAD (binary crate):

```text
cargo test homebrew_cask
# write_homebrew_cask_metadata_creates_brew_installed_layout … ok
# homebrew_cask_metadata_repair_detects_mise_orphan_only … ok
# homebrew_cask_receipt_uses_empty_uninstall_artifacts_for_api_fallback … ok
```

### Live Caskroom cohort (this machine, read-only)

| Cohort                                        |                    Count | brew `list --cask --versions TOKEN` |
| --------------------------------------------- | -----------------------: | ----------------------------------- |
| Pure mise (`.mise-cask.toml`, no `.metadata`) |                       12 | **Error: not installed**            |
| Pure brew (`.metadata`, no mise receipt)      |                       18 | OK                                  |
| Hybrid (both)                                 | 2 (`grok-build`, `kimi`) | OK                                  |

Examples pure-mise fail: `claude-code`, `1password-cli`, `codexbar`, `ghostty`.  
Bare `brew list --cask` still prints all Caskroom basenames (debris). Per-token
`--versions` is the correct installed-gate probe.

**Correlation:** brew recognition tracks **metadata caskfile**, not Caskroom
occupancy and not `.mise-cask.toml`. Dual-ledger write is the only in-repo
mechanism that flips pure-mise → brew-visible without shelling out.

### Expanded concept matrix (A–H known + I–R new)

Score: **F**ull / **P**artial / **X** fail vs G1–G7.

| ID    | Concept                                                                        | G1  | G2  | G3  | G4  | G5  | G6  | G7  | Full set?              | Verdict                       |
| ----- | ------------------------------------------------------------------------------ | --- | --- | --- | --- | --- | --- | --- | ---------------------- | ----------------------------- |
| **A** | Pour-time dual ledger (`.metadata` + `.mise-cask.toml`); status on mise ledger | F   | F*  | F   | F*  | F   | F   | F   | **Yes (identity)**     | **Winner**                    |
| B     | Single owner + disable tool brew updater                                       | F   | X   | X   | X   | F   | F   | F   | No                     | Ops under main only           |
| C     | Install those casks with real brew only                                        | X   | F   | F   | F   | P   | F   | F   | No                     | Abandons pure mise pour       |
| D     | Shell out `brew install --cask`                                                | X   | F   | F   | F   | P   | F   | X   | No                     | **HIGH-REJECT** (#10582)      |
| E     | Metadata only for simple binary casks                                          | F   | P   | P   | P   | F   | F   | F   | No                     | Ownership lottery             |
| F     | Explicit adopt/handoff command                                                 | F   | P   | P   | P   | F   | F   | F   | No                     | Future opt-in only            |
| G     | Fix third-party tools only                                                     | F   | X   | X   | X   | F   | F   | F   | No                     | Humans still broken           |
| H     | zerobrew-style private Cellar                                                  | F   | X   | X   | X   | F   | F   | F   | No                     | Not brew-visible              |
| **I** | Lazy metadata on first brew touch                                              | F   | P   | P   | P   | F   | F   | F   | No                     | Racey; needs trigger          |
| **J** | PATH proxy / brew subcommand shim                                              | F   | P   | P   | P   | F   | F   | P   | No                     | Fragile; G7 creep             |
| **K** | Separate non-brew Caskroom prefix                                              | F   | X   | X   | X   | F   | F   | F   | No                     | Honest isolation, fails G2–G4 |
| **L** | Post-pour brew register (`reinstall --force` / Ruby Tab)                       | P/X | F   | F   | F   | F   | F   | P   | No                     | Near HIGH-REJECT; needs brew  |
| **M** | Docs dual-path only (no code)                                                  | F   | X   | X   | X   | F   | F   | F   | No                     | Temporary ops                 |
| **N** | A + brew-upgrade quarantine (list yes, brew bumps no)                          | F   | P   | F   | P   | F   | F   | F   | Near                   | Optional polish of A          |
| **O** | Upstream brew reads `.mise-cask.toml`                                          | F   | F   | F   | F   | F   | F   | F   | Yes **if** brew merges | Not mise-shippable alone      |
| **P** | Non-prefix shims only                                                          | F   | X   | X   | X   | F   | F   | F   | No                     | K-lite for binaries           |
| **Q** | Emit metadata only if `brew` binary present                                    | F   | P   | P   | P   | F   | F   | F   | No                     | Gate for A, not replacement   |
| **R** | Minimal stub caskfile only (honest docs)                                       | F   | F*  | F   | F*  | F   | F   | F   | Yes (same as A)        | **Not distinct** — is A       |

\*G2/G4: installed **gate** full; full uninstall/zap/app-move lifecycle still partial.

#### New concepts — short analysis

- **I lazy:** avoids writing private API until brew is used; fails first
  Codex/startup race; needs shim or “run brew once.” Inferior to eager A.
- **J shim:** answers from mise ledger without FS identity. Breaks absolute
  path to brew, GUI, non-PATH callers. High maintenance.
- **K separate prefix:** removes path confusion; renounces “under Homebrew
  prefix” product story that formula pour already chose.
- **L post-pour brew register:** best **real** tab quality, worst purity —
  requires Homebrew present and shells to brew (or embeds private Ruby).
  Conflicts with “no Homebrew required” bootstrap story.
- **N quarantine:** refinement if dual-upgrade races dominate. Still ships A’s
  caskfile; only changes upgrade policy marker. **Does not replace A.**
- **O brew-side:** cleanest long-term purity; schedule/politics outside mise.
  Keep as aspirational; do not block A on it.
- **Q brew-present gate:** optional later; alone leaves pre-brew pours orphan
  when brew is installed later.

### Product-goal fit of recommended path

| Goal                                        | Direction A claim                  | Evidence                                      |
| ------------------------------------------- | ---------------------------------- | --------------------------------------------- |
| G1 mise Rust pour, no `brew install --cask` | Yes                                | `cask.rs` install path; #10582 alignment      |
| G2 brew-identical **install identity**      | Yes (identity, not full lifecycle) | Hybrid tokens pass brew; docs caveats         |
| G3 `brew list --cask --versions TOKEN`      | Yes after metadata                 | Live grok-build/kimi; pure-mise fail without  |
| G4 `brew upgrade --cask` installed-gate     | Yes                                | Same gate as list; E2E earlier on branch      |
| G5 mise ledger status/upgrade               | Yes                                | Status ignores missing tab; `.mise-cask.toml` |
| G6 preserve foreign `.metadata`             | Yes                                | #11012 ignore + preserve; cleanup skip        |
| G7 bootstrap-scoped                         | Yes                                | No brew rewrite; empty uninstall honesty      |

**Shell-out-to-brew (D)** scored **HIGH-REJECT** — explicit jdx non-goal.

### Why A still wins after expanded brainstorm

1. **Only mise-shippable concept that hits G1–G7 identity set without shell-out.**
2. Formula `#10326` is the positive precedent (same class of pour-time brew ledger).
3. Generate path never WONTFIX’d; preserve-only (#11012) was a different bug.
4. Live machine proves pure-mise ↔ fail and dual-ledger ↔ pass correlation.
5. New I–R either fail G2–G4, need external merge (O), or are A refinements (N/R).
6. Competing-design blanket A’s status lie is **not** part of product A (fixed).

**Do not promote to default:** B, C, D, E, G, H, I, J, K, L-as-default, M, P, Q-alone.  
**Optional later:** N (upgrade quarantine), F (explicit handoff), O (brew upstream).

### HEAD alignment

Branch `fix/brew-cask-homebrew-metadata-receipt` implements product Direction A:

- pour-time `write_homebrew_cask_metadata`
- empty `uninstall_artifacts`
- repair when `.mise-cask.toml` proves mise ownership and caskfile missing
  (upgrade driver path includes Installed packages; plain apply skips Installed)
- status does **not** use `Missing` when only brew tab is absent
- #11012-style preserve on stale version cleanup

**Gaps (document, not blockers for identity direction):**

1. Repair not on every `apply` of already-Installed packages (upgrade path yes).
2. Installed gate ≠ full lifecycle (empty uninstall tab; app copy vs brew move).
3. Dual upgrade race if user runs both mise and `brew upgrade --cask`.
4. Private Homebrew tab format coupling.
5. Upstream accept still **unproven**.

### Single recommendation (reconfirmed)

**Ship / keep potential product direction: Direction A — formula-style cask
identity** (pour-time dual ledger, no status lie).

```text
mise bootstrap brew-cask:TOKEN
  → Rust pour artifacts (G1)
  → .mise-cask.toml           # mise status / upgrade (G5)
  → .metadata/…               # brew installed? (G2–G4)
  → preserve foreign .metadata  (G6)
```

**Why not replace with a new concept:** expanded matrix (I–R) found no better
mise-only full-identity path. **O** is theoretically pure but not controllable
from this fork. **N/F** are polish/opt-in, not substitutes.

**Honest limits:** identity gate done; full offline uninstall/zap parity and
upstream acceptance deferred.

**Ops under main without A:** B + tool flags (e.g. Codex
`check_for_update_on_startup = false`) — insufficient for G2–G4 product finish.

**Process:** keep extending this file; no jdx PR/issue/comment unless policy lifts.

### Fourth-pass conclusion

Deep multi-path verification **reconfirms** Direction A. No better verified
option for G1–G7 without `brew install --cask`. Branch HEAD already implements
A’s core. Research goal: decision record extended; recommendation locked as
potential fork product direction (not proven jdx accept).

**Superseded by the fifth pass below:** the goal set survives; the empty-tab
mechanism does not.

## Fifth pass — 2026-07-23: adversarial audit and Homebrew source trace

**Trigger:** an independent advisor audit (untracked `plans/` directory)
rejected Direction A's mechanism outright. This pass adjudicated that conflict
with a line-by-line trace of Homebrew's uninstall/upgrade/loader code
(local Homebrew 6.0.12-92-g78430a5, HEAD `78430a54d`), live web verification
of the API and upstream repos, and inspection of real brew-installed metadata
on this machine. Verdict: **the plans' critique is correct on every
load-bearing point; Direction A's mechanism is refuted; a strictly better
mechanism exists (Direction A2) that keeps the entire G1–G7 goal set.**

### Refuted fourth-pass claims

| Claim                                                            | Status                           | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ---------------------------------------------------------------- | -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Empty `uninstall_artifacts` is "correct for online API fallback" | **REFUTED**                      | `cask_loader.rb:841-873`: line 842 `artifacts.presence` turns `[]` into `nil` — an empty tab list is **indistinguishable from missing metadata**. Fallback resolves artifacts from the **current** live API with **zero version comparison** (`cask_loader.rb:462-483`). Installed v1 gets uninstalled with v2's stanzas; renamed apps / changed `pkgutil` ids silently fail to remove. Offline: brew warns "files installed by the Cask may remain" (`installer.rb:549-554`), deletes its records, purges Caskroom, and **orphans the payload**. A stale `$HOMEBREW_CACHE/api/cask/<token>.json` is used silently. The official design doc states the receipt owns "the installed cask `version` and uninstallable artifacts" — Homebrew itself fixed exactly this bug class in July 2026 ([#22993](https://github.com/Homebrew/brew/issues/22993) → [#23001](https://github.com/Homebrew/brew/pull/23001), receipts now written on upgrade). Empty tab is disaster recovery, not a contract. |
| "Preserve foreign brew `.metadata`" (G6: Yes)                    | **REFUTED on HEAD**              | `write_homebrew_cask_metadata` (`cask.rs:1441-1457`) is provenance-blind: deletes **every** `.metadata` version dir and overwrites the shared tab. `installed_cask_version`'s no-receipt branch (`cask.rs:1371-1391`) counts a brew-installed cask as installed, so `bootstrap packages upgrade` over a brew-owned cask pours a new version and **destroys brew's exact tab** (zoom-class `launchctl`/`pkgutil`/`delete` stanzas lost — verified present in the real zoom tab on this machine). Branch docs' "genuine Homebrew-authored `.metadata` are left untouched" is false in this path.                                                                                                                                                                                                                                                                                                                                                                                                 |
| Repair probe is safe                                             | **PARTLY REFUTED**               | `homebrew_installed_caskfile_exists` (`cask.rs:1505-1526`) checks only `.metadata/<current-version>`; brew's installed gate globs **across all versions** (`caskroom.rb:47-62`, max timestamp basename). A brew caskfile under an older version dir is invisible to the probe → repair fires → writer deletes the brew version dir.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| "Empty is safer than partial" (code comment `cask.rs:1563-1570`) | **HALF-RIGHT, WRONG CONCLUSION** | Partial list is indeed worse than empty. But the correct fix is the **full exact list**, which mise already possesses at pour time (`Cask.artifacts: Vec<Value>` retains the raw stanzas; `json_cached` fetches the full body).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

### New load-bearing facts (all verified this pass)

1. **No historical API exists.** `formulae.brew.sh` v2 static, JWS bulk, and
   the internal `packages.<bottle_tag>.jws.json` endpoints all serve
   current-version-only; versioned URLs 404. **Pour time is the only moment
   the exact installed-version definition can be captured.** ~~This also
   bounds backfill: legacy mise-only pours can be repaired exactly only when
   `receipt.version == current API version`.~~ _Withdrawn in the sixth pass:
   version equality is not historical proof (definitions change without
   version bumps); legacy backfill from the live API is forbidden._
2. **Verbatim pour-time cask JSON is a first-class brew on-disk format.**
   Pre-2026-07-05 brew wrote the full API JSON as the installed caskfile
   (changed by `9da6c488b` "Store cask metadata as JSON"). The loader still
   honors it: `load_from_json` (`cask_loader.rb:462-483`) — when the caskfile
   has `version` + `artifacts`, **the tab is never consulted, no API call is
   made, uninstall/upgrade/zap are offline-exact and version-exact**. On next
   `brew update`, `Caskroom.migrate_caskfile_to_json` (`caskroom.rb:72-151`)
   converts it to the new minimal format with round-trip validation — the
   same migration brew applies to its own legacy installs. The struct
   generator explicitly promises backward compatibility for installed
   caskfiles (`cask_struct_generator.rb:11-12`).
3. **Real brew writes `{}` caskfiles today** (verified: 19 of 20 caskfiles on
   this machine are 2 bytes) **but pairs them with a full tab**:
   `uninstall_artifacts = artifacts_list(uninstall_only: true)` — every
   artifact with an uninstall phase, plus zap, plus flight-block summaries
   (`cask.rb:688-711`, `tab.rb:27-39`). `"artifacts": []` goes in the
   _caskfile_, and only when the uninstall list is genuinely empty
   (`installer.rb:525-527`). Mise's `{}` + `[]` inverts the convention.
4. **App-cask layout makes brew lifecycle _raise_, not merely leak.** Brew's
   Moved artifact leaves a **symlink** at the staged Caskroom source
   (`moved.rb:171-175`); `move_back` on a real directory there fails with
   "It seems there is already an App at '<source>'" (`moved.rb:198-204`).
   Mise stages a real app copy — so even with perfect metadata, non-forced
   `brew uninstall`/`brew upgrade` of a mise-poured **app** cask errors out.
   Binary artifacts are Symlinked, not Moved — unaffected. This independently
   validates binary-first lifecycle eligibility.
5. **Flight-block casks can never have JSON parity.** Brew writes a full
   `.rb` caskfile when `uninstall_preflight`/`uninstall_postflight` exist
   (`installer.rb:517-523`); JSON cannot carry Ruby. Fail closed for those.
6. **`brew list --cask --versions` reads only the caskfile path's version
   directory name** (`caskroom.rb:64-69`); file contents are irrelevant to
   the gate. `brew info` additionally reads the tab for the "Installed using
   the API" line.
7. **Dependency gap independent of metadata:** brew records cask formula
   dependencies in the tab (`runtime_dependencies`; real codex tab lists
   ripgrep+pcre2) and installs them; mise skips `depends_on` entirely
   (`cask.rs:1655`). Read by `brew deps`/`bundle`, not uninstall. A
   mise-poured codex genuinely differs from a brew install here (no ripgrep).
8. **The on-disk contract is formally private and actively moving.** Heavy
   `cask_loader.rb`/`tab.rb` churn through July 2026 (`b512bd5df`,
   `d8401141c`, `71bfac209`, `9da6c488b`, #23001); nothing changed after the
   pinned `78430a54`. No Homebrew statement for or against third-party
   metadata writers exists; the only signal is the generic "internal API,
   may change without warning" rubydoc note. Mise's formula-receipt
   coexistence has been publicly documented for months with no Homebrew
   reaction.
9. **Upstream mise:** #11197/#11198 still open, unmerged (2026-07-23);
   nothing new touching Caskroom `.metadata`/`INSTALL_RECEIPT` since
   2026-07-22.

### Direction A2 — exact pour-time snapshot (supersedes A's mechanism)

> **Sixth-pass adjustment:** the _caskfile-as-authority_ choice below was
> demoted after empirical verification and cold review (verbatim JSON is
> version-exact but not pour-exact, and brew rewrites it at the first
> `brew update` anyway). The shipping mechanism is **A3**: brew-native
> minimal installed JSON + exact projected tab; raw pour-time JSON kept as
> oracle only. Goal set and everything else in this section stands.

```text
mise bootstrap brew-cask:TOKEN
  → Rust pour artifacts (G1)
  → .mise-cask.toml                       # mise status/upgrade (G5)
  → ownership classifier gate             # plans/001 — never mutate foreign
    .metadata (G6); fixes provenance-blind writer + cross-version probe
  → .metadata/<v>/<ts>/Casks/<token>.json # VERBATIM pour-time API JSON —
    the legacy brew format; version-exact, offline-exact, auto-migrated (G2–G4)
  → .metadata/INSTALL_RECEIPT.json        # tab with real uninstall_artifacts
    mirroring artifacts_list(uninstall_only: true) semantics
  → config.json                           # target paths at uninstall
```

Why A2 strictly dominates A:

- Same goal coverage (G1–G7), same no-shell-out purity, same installed gate.
- Removes the version-mismatch, offline-orphan, and stale-cache failure modes
  — brew loads the exact installed definition from disk.
- Uses only data mise already holds at pour time (raw fetched body +
  `artifacts: Vec<Value>`); implementation is _simpler_ than synthesizing
  canonical minimal metadata.
- The caskfile is the authority (loader path 1); the tab mirror matters for
  brew's migration equivalence check and post-migration state, not the gate.
- Honest lifecycle boundary stays: binary casks get full lifecycle parity
  now; app casks stay Homebrew-invisible until mise replicates the Moved
  symlink layout (fact 4); flight-block casks fail closed permanently
  (fact 5).

**Relation to `plans/`:** plans 001 (ownership state machine), 003
(reconcile hook — fixes repair unreachability on plain apply), and 004
(per-class lifecycle gates) are adopted as written. Plan 002's _exactness
requirement_ is adopted, with its mechanism simplified: the snapshot source
is the verbatim pour-time JSON (legacy brew format), not a reconstructed
minimal shape — see the research-update note in `plans/README.md`.

### Fifth-pass conclusion

The fourth pass asked "which direction"; this pass asked "does the mechanism
survive contact with Homebrew's actual code" — it did not. The product goals
G1–G7 and the dual-ledger direction survive fully intact; every mechanical
choice on branch HEAD (`{}` + `[]`, provenance-blind writer, current-version
probe, install-path repair) is replaced by A2 + the plans' state machine.
Upstream status unchanged: not rejected, not accepted; policy still forbids
any jdx-facing PR. Next concrete step: execute `plans/001` → `002` (A2
mechanism) → `003`, then re-extend this record with E2E results.

**Superseded in part by the sixth pass:** A2's _verbatim-caskfile-as-authority_
variant was demoted after empirical verification and cold plan review; the
exactness principle and pour-time capture survive as mechanism A3 below.

## Sixth pass — 2026-07-23: empirical A2 verification and final mechanism

Two independent inputs converged this pass: (1) a sandboxed empirical run of
brew's actual migration and loader code against candidate mise metadata
(`Cask::Caskroom.path` repointed, real `migrate_caskfile_to_json` executed on
live API JSON for parallels/docker-desktop/firefox, a synthetic
`version :latest` cask, and the real tap `docker-desktop.rb`); (2) a cold
adversarial review of the fifth-pass proposal inside the plan set.

### Adjudication: caskfile-authority vs tab-authority

The fifth pass proposed the verbatim pour-time API JSON as the installed
caskfile (**caskfile-authority**). The cold review rejected it: verbatim JSON
is **version-exact but not pour-exact** — it claims artifacts mise skipped
(Codex's generated completions), carries platform `variations` re-resolved
against whatever OS loads them, and encodes source paths that may not match
mise's staged layout. The empirical run then showed the dispute is narrower
than it looked:

- Brew **rewrites the verbatim caskfile at the first `brew update`** anyway
  (`caskroom.rb:90-93`: full-JSON keys fail the `current_json` check), by the
  same migration it applies to its own pre-2026-07-05 legacy installs. The
  migrated steady state is minimal installed JSON + **tab as the durable
  artifact store**.
- Real brew's own fresh installs already write that steady state directly
  (`installer.rb:517-531`).

So both mechanisms converge on the identical durable state; caskfile-authority
merely adds a transient window that claims skipped artifacts and defers to a
migration mise does not control. **Final mechanism (call it A3, = `plans/002`
authority): write the brew-native steady state directly — minimal installed
JSON (`{}` unless the actual staged layout needs `url_specs.only_path`) plus a
non-empty exact tab projected from the filesystem actions mise actually
completed, augmented with this version's declarative uninstall/zap stanzas.**
The raw pour-time JSON is still captured — as fixture/validation input and
variation/eligibility oracle, never as installed authority. One accepted
trade-off, recorded honestly: with a `{}` caskfile there is no second artifact
source for brew to cross-heal from (migration's self-heal only operates when
the caskfile carries artifacts), so golden fixtures + drift tests carry that
burden (`plans/002` steps 3, `plans/003` step 6).

### Empirical results that harden the plans (binding facts)

| #   | Fact                                                                                                                                                                                | Evidence                                                                                                                                                                                                                                                                             | Consequence                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Migration failure mode is safe: loud `opoo`, original caskfile preserved, cask stays uninstallable                                                                                  | `caskroom.rb:129-149`, `update-report.rb:48-49`; round-trip compare is order-insensitive multiset equality (`artifacts_equivalent?`, `caskroom.rb:153-156`)                                                                                                                          | No silent-corruption path through `brew update`                                                                                                                                                                                                                                                                                                                                                                                    |
| 2   | Tab≠caskfile mismatch never errors; caskfile artifacts win and get baked into installed JSON; tab is never rewritten                                                                | `caskroom.rb:123-126`; empirically confirmed with subset tab                                                                                                                                                                                                                         | Tab projection bugs are recoverable, not destructive — but produce non-native shape; fixtures must mirror `artifacts_list(uninstall_only: true)` (`cask.rb:687-711`) exactly, including app/binary entries, zap, and flight-marker stubs                                                                                                                                                                                           |
| 3   | `homebrew_version: "5.1.15 (mise)"` is **safe**                                                                                                                                     | Only parser `tab.rb:171-176` → `Version.new` (parses it); **zero consumers in cask code paths**; formula gates evaluate correctly                                                                                                                                                    | Worry closed; keep the string, bump base occasionally                                                                                                                                                                                                                                                                                                                                                                              |
| 4   | `depends_on` cannot brick uninstall after an OS upgrade                                                                                                                             | `cask_loader.rb:545-553` rescues `MacOSVersion::Error`; requirements enforced only in install/upgrade prelude                                                                                                                                                                        | Worry closed                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 5   | Flight-block casks as JSON: uninstall blocks **silently no-op** (EMPTY_BLOCK stubs), and migration skips such files forever                                                         | `cask_struct_generator.rb:150-152`, `caskroom.rb:110`; empirical                                                                                                                                                                                                                     | JSON parity impossible for `uninstall_preflight`/`postflight` casks under _any_ JSON mechanism — matches brew's own rule of writing `.rb` for them                                                                                                                                                                                                                                                                                 |
| 6   | **Verbatim tap `.rb` as installed caskfile works end-to-end** for flight-block casks                                                                                                | Empirical with real `docker-desktop.rb`: loads via `instance_eval`, real Ruby blocks (not stubs), trusted (official tap implicitly trusted, `tap.rb:1466-1468`; Caskroom allowlisted, `utils/path.rb:150-160`), untapped OK via `tab.source.tap`, migration leaves it byte-identical | Future `plans/004` gate: flight-block casks _can_ reach full parity via checksum-verified version-matched `.rb` (mise already fetches it: `ruby_source_path`/`ruby_source_checksum`, `fetch_cask_rb` `cask.rs:478`). Hard conditions: exactly one caskfile per timestamp dir (stale `.json` beats `.rb`, `caskroom.rb:14,56-58`); filename == token; tab carries `source.tap`, `uninstall_flight_blocks: true`, full artifact list |
| 7   | Timestamp dir name must parse as `%Y%m%d%H%M%S.%L` — `Time.strptime` raises otherwise, breaking `brew info --json`                                                                  | `cask.rb:287-292`, `metadata.rb:13`                                                                                                                                                                                                                                                  | Mise's existing format matches; pin with a test                                                                                                                                                                                                                                                                                                                                                                                    |
| 8   | Mise-private files at `Caskroom/<token>/` root or `.metadata/` root block dir removal on brew uninstall → `brew doctor` flags "corrupt" and suggests destructive repair             | `installer.rb:740-761`, `caskroom.rb:159-168`, `diagnostic.rb:1188-1200`                                                                                                                                                                                                             | `.mise-cask.toml` inside the version dir is safe (purged with it); never add token-root or metadata-root mise files                                                                                                                                                                                                                                                                                                                |
| 9   | `brew cleanup` never touches unknown files in version dirs or `.metadata`; cask-side autoremove reads the _loaded cask's_ `depends_on`, never the cask tab's `runtime_dependencies` | `cleanup.rb:492-500,775-787,878-890`; `utils/autoremove.rb:27-51`                                                                                                                                                                                                                    | Cleanup-safety worry closed; tab deps are honesty/bookkeeping (`plans/006`), nothing consumes them today                                                                                                                                                                                                                                                                                                                           |
| 10  | `version :latest` loads fine (dir name `latest`); greedy upgrade uses `LATEST_DOWNLOAD_SHA256`                                                                                      | `dsl.rb:505-514,679-684`, `cask.rb:374-377,425-429`                                                                                                                                                                                                                                  | Deferred to dedicated fixtures (`plans/002` excludes it from v1)                                                                                                                                                                                                                                                                                                                                                                   |
| 11  | Variations and `language_variations` are re-resolved at every load against the current OS/config                                                                                    | `api.rb:172-183`, `cask_loader.rb:519-522` (languages added 2026-07-17, `4880cf92d`)                                                                                                                                                                                                 | Tab projection from pour-resolved actions sidesteps OS-upgrade drift; config.json should carry `languages` for non-default-language casks                                                                                                                                                                                                                                                                                          |

### Corrections to this record from the cold review

1. **Fifth-pass backfill exception withdrawn.** The version-match rule
   ("current API JSON is exact when `receipt.version == current`") is not
   historical proof: definitions change without version bumps, and target
   existence proves neither skipped artifacts nor hooks nor dependency
   closure. `plans/003` now flatly forbids legacy lifecycle backfill from the
   live API; legacy pours wait for a real upgrade/reinstall to capture exact
   data. The stricter rule stands.
2. **`depends_on` upgraded from "track separately" to eligibility
   prerequisite.** `plans/006` (now P1, before 002) models constraints,
   dependency ownership (`installed_on_request` provenance), and the resolved
   closure; a dependency-bearing cask like Codex is not interop-eligible
   until it lands.
3. **Plan set extended to eight:** 007 makes mise-driven upgrades of interop
   casks a single recoverable transaction across payload + both ledgers
   (metadata published last, prior Homebrew authority revoked before first
   payload mutation); 008 gives every stranded ownership state an explicit,
   confirmed, state-specific recovery path (no more "delete the metadata by
   hand" as the recovery contract). Interop emission itself goes behind an
   experimental default-off setting (`plans/002` step 5) — correct for a
   formally private, actively churning Homebrew surface.

### Post-takeover convergence (local trace, this pass)

After a real `brew upgrade --cask` of a mise-poured cask: brew purges the old
version dir including `.mise-cask.toml`, leaving one brew-owned version dir.
`installed_version` (`cask.rs:1229-1251`) then reports that single dir; the
no-receipt branch verifies artifact targets; mise reports Installed at brew's
version; repair is a no-op without a mise receipt. Clean `HomebrewOwned`
transition, matching `plans/003` step 4's expectation. Residual edge: a
multi-version-dir Caskroom makes mise report Missing and reinstall-over —
`plans/001`'s classifier must treat multi-version state as `Conflict`, not
Missing.

### Sixth-pass conclusion

**Final locked mechanism: A3 — brew-native minimal installed JSON + exact
projected tab, pour-time raw JSON as oracle only, ownership state machine
first, experimental-gated, binary casks first.** This is Direction A's goal
set (G1–G7 unchanged), the fifth pass's exactness principle, and the plan
set's safety architecture in one line. Verified safe against brew's actual
migration/loader by execution, not argument. Flight-block casks have a
verified future path (`.rb`); app casks wait on Moved-symlink layout parity;
`version :latest` waits on dedicated fixtures.

**Execution order correction (seventh pass):** the one-liner below had
`003 → 008`; `plans/README.md` Depends-on graph requires recovery before full
lifecycle E2E: `001 → 006 → 002 → 007 → 008 → 003`, then 004 per class, then
005 ADR; Plan 009 parallel long-term (not a hard blocker for experimental
A3). Upstream posture unchanged: not rejected, not accepted, no jdx-facing
PR without an explicit policy lift.

---

## Seventh pass — 2026-07-23: deep research reconfirm and direction lock

**Trigger:** re-open the decision record with multi-agent brainstorm, live
Homebrew re-verify, adversarial attack on A3/`plans/001–009`, and fix
document debt (executive still said “Ship A2” while body locked A3).

**Pin:** Homebrew `6.0.12-92-g78430a5` @ `78430a54dd972a9725cf5f9a862bacd330303906`
(`/opt/homebrew`). Mise branch `fix/brew-cask-homebrew-metadata-receipt` @
research parent `866916893` (working tree may carry this findings edit only).

**Agents used (5 parallel + parent traces):**

| Role                                      | Artifact (session scratch)                                       |
| ----------------------------------------- | ---------------------------------------------------------------- |
| Homebrew source re-verify (claims 1–10)   | `homebrew_trace.txt`, `claim_matrix.md`                          |
| Adversarial A3 critique                   | `adversarial_a3.md`                                              |
| Direction matrix vs G1–G7 (≥8 directions) | `direction_matrix.md`                                            |
| plans/001–009 consistency audit           | `plans_audit.md`                                                 |
| Live Caskroom / disk sample               | `live_brew_sample.txt`, `live_cask_sample.txt`                   |
| Parent CLI + line dumps                   | `live_brew_cli.txt`, `claim_evidence_detail.txt`, `subagents.md` |

### Claim re-verification (load-bearing)

All ten load-bearing claims from the sixth-pass stack were re-checked against
live brew source and this machine’s Caskroom. **None REFUTED.** Nuances only.

| #   | Claim                                                                                         | Status                 | Evidence (brew @ `78430a54`)                                                                                                                     | A3 consequence                                                                                                                             |
| --- | --------------------------------------------------------------------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Installed gate = caskfile under `.metadata/*/*` (glob all versions, max timestamp basename)   | **CONFIRMED**          | `caskroom.rb` `cask_installed_caskfile`; CLI: `brew list --cask --versions codex` → `0.145.0`; ghostty (payload, no `.metadata`) → not installed | Emit any valid-path caskfile for G3; repair probe must be **cross-version**, not current-version-only (`cask.rs:1505-1526` still narrower) |
| 2   | Empty tab `uninstall_artifacts` / `artifacts.presence` ≡ missing → live API, no version check | **CONFIRMED**          | `cask_loader.rb:841-873` `artifacts.presence` then `cask_json(token)["artifacts"]`; offline orphan path `installer.rb:549+`                      | **Never ship `[]` as product.** HEAD still does (`cask.rs:1563-1588`)                                                                      |
| 3   | Migration: full JSON → minimal; durable artifact store is tab under steady-state `{}`         | **CONFIRMED**          | `migrate_caskfile_to_json` `caskroom.rb:72-151`; early-out when `current_json` + tab present; migrator **does not rewrite tab**                  | Write A3 steady state directly; do not depend on A2 transient full JSON                                                                    |
| 4   | Real brew: `{}` caskfile + **full** tab                                                       | **CONFIRMED**          | Live: codex `{}` + tab len 3; zoom `{}` + len 2; bartender/vlc same. `Tab.create` `tab.rb:36` = `artifacts_list(uninstall_only: true)`           | A3 matches brew; HEAD inverted (grok-build: mise tab `[]`, `homebrew_version` `5.1.15 (mise)`; caskfile later filled by migration)         |
| 5   | App Moved: Caskroom source must be **symlink**; real dir breaks `move_back`                   | **CONFIRMED**          | `moved.rb:171-175`, `198-204`                                                                                                                    | App casks ineligible for full lifecycle until layout parity                                                                                |
| 6   | Flight-block uninstall: brew writes `.rb`, not JSON                                           | **CONFIRMED**          | `installer.rb` flight branch; migration skips flight casks                                                                                       | JSON emit fail-closed; future `.rb` path (sixth-pass fact 6)                                                                               |
| 7   | Timestamp `%Y%m%d%H%M%S.%L`                                                                   | **CONFIRMED** (scoped) | Parse needed for `install_time` / info JSON; gate uses string `max_by` only                                                                      | Keep UTC ms formatter; pin unit test                                                                                                       |
| 8   | Token-root / metadata-root extra files block purge → doctor “corrupt”                         | **CONFIRMED**          | `installer.rb` rmdir; version-dir `.mise-cask.toml` is safe                                                                                      | Never place mise files at token or `.metadata` root                                                                                        |
| 9   | No historical cask API on formulae.brew.sh                                                    | **CONFIRMED**          | Current-only endpoints; version-blind recovery                                                                                                   | Pour-time (or reinstall) only; **forbid** live-API lifecycle backfill                                                                      |
| 10  | Tab≠caskfile: caskfile artifacts win when present; tab not rewritten                          | **CONFIRMED**          | Loader prefers caskfile artifacts; pure `{}` → tab sole authority                                                                                | A3 has **no** brew self-heal of wrong tab — golden fixtures + real uninstall E2E required                                                  |

**Live CLI (this pass):** `brew list --cask --versions codex` / `grok-build` both report versions; `brew info --cask --json=v2` reports `installed` for both. That proves the **gate**, not lifecycle safety for empty-tab mise hybrids.

### Concept / direction matrix (scored vs G1–G7)

Scoring: **Y** / **P** / **N**. Full identity set = honest G2–G4 for the
product surface users care about without abandoning G1.

| #   | Direction                                     | G1  | G2  | G3  | G4  | G5  | G6  | G7  | Verdict                                                                    |
| --- | --------------------------------------------- | :-: | :-: | :-: | :-: | :-: | :-: | :-: | -------------------------------------------------------------------------- |
| 1   | Status-quo empty tab (HEAD)                   |  Y  |  N  |  Y  |  P  |  Y  |  N  |  Y  | **KILL** — empty tab ≡ missing; provenance-blind writer destroys brew tabs |
| 2   | A2 verbatim pour-time caskfile authority      |  Y  |  P  |  Y  |  P  |  Y  |  P  |  Y  | **KILL** — not pour-exact; brew rewrites; authorizes skipped artifacts     |
| 3   | **A3** min JSON + exact projected tab         |  Y  |  P  |  P  |  P  |  Y  | Y†  |  Y  | **WIN (near-term experimental)**                                           |
| 4   | Shell-out `brew install --cask`               |  N  |  Y  |  Y  |  Y  |  P  |  Y  |  P  | **KILL** — fails G1; #10582 non-goal                                       |
| 5   | Mise-only ledger + doc “brew won’t see you”   |  Y  |  N  |  N  |  N  |  Y  |  Y  |  Y  | **KILL as finish**; valid ops fallback / main status quo                   |
| 6   | Ownership SM only (no metadata emit)          |  Y  |  N  |  N  |  N  |  Y  |  Y  |  Y  | Foundation (`plans/001`) — **not** product alone                           |
| 7   | Binary-only A3 emit; apps mise-only           |  Y  |  P  |  P  |  P  |  Y  | Y†  |  Y  | **A3 v1 scope** (same mechanism, class gate)                               |
| 8   | Upstream-first only after Plan 009 public API |  Y  | Y‡  | Y‡  | Y‡  |  Y  |  Y  |  Y  | **Long-term preferred durable**; not sole near-term                        |
| 9   | Hybrid: mise pours; shell lifecycle           |  P  |  P  |  P  |  P  |  Y  |  P  |  P  | **KILL** — dual complexity, no clean ownership                             |
| 10  | Emit only when `brew` binary present          |  Y  |  P  |  P  |  P  |  Y  |  P  |  Y  | Optional **gate on A3**, not a model                                       |

† G6 Yes only with `plans/001` classifier (HEAD writer alone is **No**).
‡ G2–G4 Yes only after Homebrew ships a public registration contract.

**Honest G2–G4 reading (binding):** Homebrew has **no identity-only marker**.
The same caskfile that makes `list --versions` work authorizes upgrade /
uninstall / zap. Therefore G2–G4 are **class-scoped and experimental** under
A3, not universal promises. `plans/README` already corrects this; early
sections of this file that still read “identity done without lifecycle” are
historical and must not drive ship decisions.

### Adversarial critique of A3 (does not flip the lock)

| Attack                                             |               Sev               | Kills A3?                                          | Mitigation if keep                                                                                                             |
| -------------------------------------------------- | :-----------------------------: | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Private API / tab bitrot                           |                H                | No as experimental; yes as durable default promise | Default-off; pin brew version allowlist; fail-closed on unknown schema; Plan 009 exit ramp                                     |
| Wrong projected tab → destructive uninstall        |                H                | Can kill trust                                     | Live-brew-generated golden fixtures; real `brew uninstall`/`upgrade` E2E on disposable prefix; never partial “best effort” tab |
| Dual-manager race (mise vs brew concurrent)        |                H                | Kills “concurrent-safe” claim only                 | Docs: single manager for upgrade; provenance → `Externalized`; 001/007/008 recovery; no shared lock until 009                  |
| `{}` caskfile loses second artifact source         |                M                | No (accepted)                                      | Fixtures + E2E; oracle only inside mise receipt — never reintroduce A2 for “heal”                                              |
| Shell-out simpler for Codex pain alone             |                M                | Only if G1 renegotiated                            | Reject under stated goals; optional future “install via brew” command is a different product                                   |
| Formula-style slogan overclaims incomplete surface | H (product fraud if default-on) | No if wording fixed                                | Market **cooperative experimental interop for eligible binaries**, not “full brew replacement”                                 |
| Experimental forever / never ships value           |                M                | Product risk, not mechanism kill                   | Graduate-or-kill criteria in 005; keep 009 parallel                                                                            |
| Codex still ineligible until `depends_on`          |                M                | Scope honesty                                      | Plan 006 before Codex interop eligibility                                                                                      |

**Successor if A3 fails in execution:** Direction **M (mise-primary)** — dual
ledger abandoned; ownership classifier + preserve foreign brew metadata; G2–G4
only via Plan 009 or explicit user `brew install --cask`. **Never** fall back
to empty-tab A or verbatim-authority A2.

### Document debt fixed this pass

| Bug                                                              | Fix                                             |
| ---------------------------------------------------------------- | ----------------------------------------------- |
| Executive still “Ship **A2**” while sixth pass locks **A3**      | Executive rewritten to A3                       |
| Header chain understated A3 / still “A reconfirmed” as mechanism | Header lists A → A2 → **A3** supersession       |
| Sixth-pass execution one-liner `003 → 008` vs plans Depends-on   | Corrected to `… → 008 → 003`                    |
| Branch HEAD empty-tab still documented as if acceptable          | Explicit: **do not ship HEAD empty-tab writer** |
| G2 framed as “identity only” without lifecycle caveat            | Corrected in executive table + this pass        |

### Plans alignment

| Check                                                           | Result                                                                                                                      |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Plan 002 authority = A3 (min JSON + exact tab; raw JSON oracle) | **Aligned** — no plan ships A2 or empty tab                                                                                 |
| Execution order                                                 | **Trust `plans/README`:** `001 → 006 → 002 → 007 → 008 → 003 → 004 → 005`; **009 parallel**                                 |
| Gaps deferred (not fake-solved)                                 | App Moved-symlink; flight-block `.rb`; dual-upgrade race; private-API churn; jdx accept; universal G2–G4; live-API backfill |
| Plan 009                                                        | **Not** hard blocker for experimental A3; **is** blocker for robust default-on G2–G4                                        |

### Recommended direction (seventh-pass lock)

```text
Direction A (goal set) + Mechanism A3
  ownership SM (001)
  → depends_on / constraints (006)
  → emit brew-native {} + exact tab for eligible binaries only (002), experimental default-off
  → transactional mise upgrades (007)
  → recovery matrix (008)
  → reconcile + real brew lifecycle E2E (003)
  → expand classes (004) when layout/parity proven
  → ADR consolidate (005)
  ∥ Plan 009 public registration design (no jdx/Homebrew contact without policy lift)
```

**Why A3 still wins after adversarial re-open:**

1. Only dual-ledger path that matches **brew’s actual steady state** without
   lying about skipped artifacts.
2. Empty tab and A2 are **mechanically worse**, not taste preferences — re-proven
   this pass on live brew source and hybrid `grok-build` disk state.
3. Shell-out and mise-only fail product goals as stated (G1 or G2–G4).
4. Risks that remain are **execution risks** (projection correctness, bitrot,
   dual-manager docs) mitigated by experimental gates + plans, not by switching
   mechanism.

**What is still deferred (must not pretend solved):**

- Universal G2–G4 for all cask classes
- Codex interop until 006 lands dependency ownership
- App casks until Moved-symlink layout
- Flight-block JSON parity (impossible; `.rb` later)
- Concurrent mise+brew upgrade safety
- Default-on without 009 or multi-version brew pin matrix
- Upstream jdx accept; any jdx-facing PR
- Legacy hybrid cleanup (`grok-build`/`kimi` empty tabs) beyond 008 recovery ops

### Seventh-pass conclusion

**Reconfirm: A3 is the correct near-term mechanism; Direction A’s G1–G7 intent
remains the product target with class-scoped honesty.** Supersession chain:
**A (empty tab) → A2 (verbatim caskfile) → A3 (minimal JSON + exact tab).**
No better verified local alternative without abandoning G1 or inventing a
Homebrew public API (009). Branch HEAD metadata writer remains **unsafe to
ship**. Next work is plan execution, not further mechanism shopping — unless
live brew source flips a claim in the matrix above.

**Fork policy unchanged:** research/decision record only; no jdx/mise
PR/issue/comment authorization from this work.

---

## Eighth pass — 2026-07-23: architecture correction and supported handoff discovery

### Trigger and verdict

The seventh pass proved that A3 is the least-wrong way to serialize Homebrew's
current private installed-cask format. It did **not** prove that publishing that
format creates a safe product architecture. Three independent audits attacked
that distinction, then the parent trace verified their evidence.

**Corrected verdict:**

1. **Default:** mise-owned direct pour, no synthetic Homebrew metadata.
2. **Best current interop candidate:** explicit one-way handoff through the
   supported `brew install --cask --adopt` command. Homebrew writes its own
   metadata and becomes sole lifecycle owner. This needs a disposable E2E before
   adoption as product direction; it is not yet proven against mise's staged
   shape or failure recovery.
3. **Strict no-`brew` interop:** presently unsatisfied as a robust product
   contract. A3 may remain a default-off binary-only research adapter, but marker
   publication must be treated as ownership transfer, not shared identity.
4. **Durable solution:** a Homebrew-supported registration/handoff API with
   Homebrew validation, locking, capability negotiation, and compare-and-swap.
5. **Never return to:** empty tab, verbatim API authority, live-API backfill,
   identity-only language, or uncoordinated dual writers.

This changes the product recommendation, not the A3 serialization finding. A3
is still the correct shape **if** a private-format experiment proceeds: minimal
installed JSON plus exact non-empty tab derived from completed actions. It is no
longer described as “locked,” “ship,” or complete coexistence.

### Current upstream re-verification

Homebrew remote HEAD at research time was
[`33c3da5f`](https://github.com/Homebrew/brew/tree/33c3da5f49885a8e19170935f6e8515a66516cff),
dated 2026-07-22 19:37 UTC. The local checkout was `78430a54`; GitHub's compare
API showed only four intervening commits and only formula-related files changed.
All cited cask loader/tab/installer/Caskroom behavior therefore still matches
current remote HEAD.

Context7 was requested by repository policy but unavailable in this session.
Verification used official Homebrew documentation, current upstream source,
GitHub API/search, local current-equivalent source, and focused mise tests.

### Missed supported primitive: `brew install --cask --adopt`

The prior record said no adoption command existed. That was too broad.
Homebrew officially documents:

```sh
brew install --cask --adopt <token>
```

Official evidence:

- [Homebrew Tips and Tricks: adopt a manually installed app](https://docs.brew.sh/Tips-and-Tricks#adopt-a-manually-installed-app)
- [Homebrew manpage `--adopt`](https://docs.brew.sh/Manpage#install-options-formulacask-)
- [`install.rb` passes `adopt:` to `Cask::Installer`](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cmd/install.rb#L417-L430)
- [`Moved` verifies/adopts an existing destination](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cask/artifact/moved.rb#L73-L175)
- [`Symlinked` accepts a compatible existing Caskroom link](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cask/artifact/symlinked.rb#L67-L92)

`--adopt` is **not** a receipt-only registration API:

- Homebrew fetches dependencies and the cask archive;
- stages/extracts into the canonical Caskroom;
- activates or adopts artifacts;
- then creates the installed caskfile and tab itself
  ([`installer.rb:159-190`](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cask/installer.rb#L159-L190));
- extraction can merge with the existing same-version Caskroom directory;
- a stage failure purges versioned files, so a mise payload/receipt needs a
  reversible pre-handoff bundle before testing this on real state;
- `Moved` skips strict equality for `auto_updates`; Homebrew's own test adopts
  differing app contents
  ([`app_spec.rb:86-155`](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/test/cask/artifact/app_spec.rb#L86-L155)),
  so blind adoption can preserve an older payload under a current-version tab;
- mise's `.mise-cask.toml` may survive a successful merge and must be retired or
  marked observational only after the Homebrew-authored ledger validates.

Thus `--adopt` trades away strict G1 during handoff, but it uses a supported
Homebrew surface and makes Homebrew author its own lifecycle authority. That is
architecturally stronger than mise writing a private schema. It must be tested
on a disposable macOS runner with unique binary and app fixtures, actual
upgrade/uninstall, dependency behavior, and injected failure. No such mutation
was run on this developer machine.

`brew tab` is not an alternative registration mechanism. It edits request
provenance only after `Cask#installed?` succeeds; it cannot create the missing
installed caskfile gate
([`cmd/tab.rb:44-90`](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cmd/tab.rb#L44-L90)).

### Why local dual-writer safety remains impossible

Homebrew defines `CaskLock`, but the current install/upgrade/uninstall paths do
not acquire it. An exhaustive source search found its production use only in
cleanup lockfile handling:

- [`CaskLock` definition](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/lock_file/cask_lock.rb)
- [cleanup reference](https://github.com/Homebrew/brew/blob/33c3da5f49885a8e19170935f6e8515a66516cff/Library/Homebrew/cleanup.rb#L489-L500)

Mise's per-token lock is therefore invisible to Homebrew. Atomic rename can
make a marker appear all-at-once, but cannot prevent a Homebrew process that
loaded old authority before the rename from uninstalling while mise switches
payload. Fingerprint checks detect some aftermath; they do not prevent damage.

This makes original G4 + G5 ambiguous. “Either manager may upgrade, just not at
the same time” is advice, not an enforceable invariant. Correct near-term model:

```text
MiseOwned
  -- explicit handoff begins --> HandoffPending
  -- Homebrew authority validates/publishes --> HomebrewOwned

HomebrewOwned
  -- mise status --> observe/preserve
  -- mise upgrade --> refuse before mutation
  -- return to mise --> exact Homebrew uninstall, verify Absent, fresh mise pour
```

A3 marker publication, if retained experimentally, is the handoff
linearization point. After that point mise must not remain a second lifecycle
writer. Plan 007's mise-driven upgrade of already-visible A3 state is deferred
until a mutually honored coordination contract exists.

### Root architecture missed by the A3 plans

Current cask install has no authoritative completed-action transaction:

1. Resolve current API into `Cask`/`CaskArtifacts`.
2. Run hooks and install app/pkg/font/binary work.
3. Write `.mise-cask.toml` from intended artifact targets.
4. Rename the version directory into place.
5. Link targets and remove stale versions.
6. Publish Homebrew metadata last.

Evidence: `src/system/packages/brew/cask.rs:103-190` and `:1395-1413`.

A crash after Step 3 or 4 can leave an incomplete install that resembles a
healthy `MiseOnly` state. Plan 002 currently constructs an “exact” snapshot from
`Cask`, `CaskArtifacts`, and filesystem checks. Those are plan inputs and
observations, not proof that each action completed.

Required structural base:

```text
Raw metadata
  -> validated ResolvedCaskPlan
  -> durable transaction intent
  -> installer-emitted CompletedAction records
  -> validated CompletedActionManifest
  -> committed mise receipt/status
  -> optional one-way Homebrew handoff adapter
```

The journal must cover fresh installs, dependencies, payload, targets, receipt,
and handoff—not only A3 upgrades. It belongs under a same-volume mise recovery
root outside Homebrew-controlled version directories. Every durable phase needs
file `sync_all`, directory sync, atomic/no-clobber publication, destination
parent sync, and fault tests. `crate::file::write` is raw `fs::write`
(`src/file.rs:306-310`); `sync_dir` already exists at `src/file.rs:437-466`.

### Security prerequisite: source-derived paths are not contained

This audit found a pre-existing security/correctness issue that must precede
interop work:

- API-returned `token` and `version` are deserialized without validation or
  equality check against the request (`cask.rs:299-333`).
- Those values enter cache/extraction names (`:335-368`) and destructive
  Caskroom joins (`:1603-1628`).
- absolute app targets are checked by lexical prefix but do not reject parent
  components (`:1149-1163`).
- existing plans normalize provenance/journal relative paths only after these
  earlier I/O boundaries.

Malformed or hostile third-party tap metadata can therefore escape intended
cache/Caskroom/application roots. The fix plan must:

1. represent token/version as validated opaque types before any path creation;
2. require each filesystem component to be one normal, non-absolute component;
3. require API canonical token to match the requested token/alias result;
4. hash untrusted identifiers in cache filenames instead of interpolating them;
5. normalize and contain every app/binary/font target under an allowed root;
6. reject parent components and ancestor symlink escapes;
7. perform destructive operations relative to validated directory handles where
   feasible, with temp-prefix tests for traversal and symlink races.

No exploit was executed. Finding comes directly from data flow into destructive
filesystem functions.

### Corrected product goals

Original G1-G7 conflated Homebrew visibility with simultaneous ownership.
Replace them with:

| Goal | Requirement                                                                        |
| ---- | ---------------------------------------------------------------------------------- |
| G1   | Normal mise install/status works without invoking or requiring Homebrew            |
| G2   | Homebrew recognition exists only when Homebrew can safely own lifecycle            |
| G3   | Every supported visible state passes real `brew list/info`, not just a path gate   |
| G4   | Real upgrade, uninstall, reinstall, and applicable zap work for the declared class |
| G5   | Exactly one mutation owner exists; other managers only observe/preserve            |
| G6   | Foreign, ambiguous, partial, or unproven state is never overwritten                |
| G7   | Bootstrap/handoff scope is explicit; no general Homebrew replacement claim         |
| G8   | Concurrency uses mutually honored CAS/locking or explicit ownership transfer       |
| G9   | Durable compatibility relies on a supported/versioned interface, not private bytes |

G1 and supported `--adopt` cannot both hold **during handoff** because adoption
is a full Homebrew install path. G1 and robust G2-G4 can coexist only if
Homebrew later offers external registration, or if A3 remains an explicitly
unsupported/private experiment. This is a proven capability boundary, not an
effort judgment.

### Re-scored direction matrix

`Y` = fully satisfies corrected goal; `P` = conditional/partial; `N` = fails.

| Direction                                | G1  | G2-G4 | G5-G6 | G8-G9 | Verdict                                            |
| ---------------------------------------- | :-: | :---: | :---: | :---: | -------------------------------------------------- |
| Mise-only, no metadata                   |  Y  |   N   |   Y   |   Y   | **Safe default now**                               |
| Empty tab / live API fallback            |  Y  |   N   |   N   |   N   | **Rejected**                                       |
| A2 full API installed authority          |  Y  |   P   |   N   |   N   | **Rejected**                                       |
| Blanket A3 dual ownership                |  Y  |   P   |   P   |   N   | **Rejected as product**                            |
| Binary-only A3, one-way handoff          |  Y  |   P   |   Y   |   P   | **Private experiment only**                        |
| Local ownership lease/lock               |  Y  |   N   |   P   |   N   | **Rejected; Homebrew ignores it**                  |
| Wrapper intercepting `brew upgrade`      |  P  |   N   |   P   |   N   | **Rejected; false identity/path coupling remains** |
| `brew install --cask --adopt` handoff    |  P  |   Y   |   Y   |   Y   | **Best supported current candidate; E2E required** |
| Delegate entire cask install to Homebrew |  N  |   Y   |   Y   |   Y   | Correct alternate mode, violates strict G1         |
| Upstream external registration/CAS       |  Y  |   Y   |   Y   |   Y   | **Durable target; unavailable today**              |

Moving direct casks out of Homebrew's prefix would remove path-based false
ownership, but it changes bootstrap layout/PATH/application semantics and still
does not deliver G2-G4. Treat that as a separate product redesign, not a receipt
fix.

### Plan-set corrections

| Existing plan          | Required correction                                                                                                                                                   |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 001 ownership          | Model payload owner, lifecycle-marker owner, mutation authority, handoff phase, and contract version separately; wire results into read-only status/health            |
| 002 A3 metadata        | Consume `CompletedActionManifest`, never inferred `CaskArtifacts`; add durable fsync + no-clobber publication; one-way handoff only; no product “coexistence” wording |
| 003 reconciliation/E2E | Split non-skippable disposable harness and adoption/A3 falsification spike earlier than large implementation                                                          |
| 004 artifact expansion | Conditional on chosen ownership path and actual class lifecycle E2E                                                                                                   |
| 005 docs               | Run immediate unsafe-guidance correction first; final concise ADR/archive remains last                                                                                |
| 006 dependencies       | Put dependency graph/provenance/locks/rollback in the same root transaction; test definition drift/autoremove                                                         |
| 007 upgrades           | Defer mise mutation of Homebrew-visible state until supported CAS/lock exists; retain only as research or redesign as handoff recovery                                |
| 008 recovery           | Add healthy withdrawal/reverse-handoff rules; never auto-delete synthetic or foreign metadata                                                                         |
| 009 upstream           | Begin with supported `--adopt` capability spike; propose new API only for gaps adoption cannot solve                                                                  |
| 010 immediate safety   | Remove unconditional writer/repair and correct active docs; preserve existing trees                                                                                   |
| 011 path boundary      | Validate opaque identifiers and every artifact target before any I/O or destructive action                                                                            |
| 012 native handoff     | Prove `--adopt`/native reinstall in deterministic disposable lifecycle and failure matrices                                                                           |
| 013 completed actions  | Make mutators emit durable historical truth; final receipt only after complete activation                                                                             |

Recommended execution order:

1. **Immediate safety:** disable/remove unconditional empty-tab publication and
   repair; correct active user/dev docs; preserve existing hybrid trees.
2. **Path boundary:** validate all source-derived identifiers/targets before I/O.
3. **Early falsification:** build non-skippable disposable Homebrew harness and
   test `--adopt` with binary/app, dependencies, actual v1→v2 upgrade,
   uninstall, failure, and rollback.
4. **Decision gate:**
   - adoption acceptable: design explicit Homebrew-owned mode/handoff;
   - strict no-`brew`: keep mise-only while Plan 009 pursues registration;
   - A3 research retained: continue only as default-off binary handoff.
5. **Core transaction:** completed-action manifest, durable fresh-install
   journal, status/health, ownership authority.
6. **Dependencies:** exact graph, deterministic locks, provenance, rollback.
7. **A3 experiment:** exact minimal JSON + projected non-empty tab from completed
   actions; publication transfers mutation authority to Homebrew.
8. **Recovery:** reversible bundles, pending phases, legacy hybrid handling,
   explicit reverse handoff.
9. **Lifecycle gates:** real list/info/upgrade/uninstall/offline/fault/race tests;
   then class-by-class expansion.
10. **Durable contract:** upstream registration/CAS; only this can restore safe
    coordinated mise upgrades after Homebrew visibility.
11. **Documentation consolidation:** one concise current ADR; archive this
    append-only record so historical contradictions cannot direct executors.

### Verification performed in this pass

- Spawned three independent read-only audits: upstream Homebrew, local mise
  architecture/tests, and adversarial concept/plan review.
- Confirmed local Homebrew `6.0.12-92-g78430a5`; remote HEAD `33c3da5f`; no cask
  file changed between them.
- Read current Homebrew install/adopt/stage/tab/uninstall/lock code and official
  `--adopt`, installed JSON, and private `Cask::Tab` documentation.
- GitHub issue/PR/code searches found no supported external cask registration
  API or open proposal matching it.
- Current Codex API still declares binary + generated completions + zap and the
  `ripgrep` formula dependency; A3 cannot blindly copy its artifact array.
- Re-ran
  `rtk proxy /Users/donbeave/.cargo/bin/cargo test system::packages::brew::cask`:
  **63 passed**. This is not a safety proof: two passing tests positively assert
  the rejected empty-tab behavior.
- Did not mutate `/opt/homebrew`, `/Applications`, package receipts, taps, or any
  upstream repository.

### Final eighth-pass conclusion

**A3 solved a serialization question, not the ownership problem.** Homebrew
recognition is lifecycle authority, and no local primitive coordinates both
managers. Therefore current correct default is mise-only. Best present
interop research is a supported, explicit `brew install --cask --adopt`
ownership handoff with
Homebrew as sole mutator. A3 is retained only as a private binary handoff
experiment if strict no-`brew` remains mandatory. Robust zero-subprocess
interop requires an upstream registration/CAS contract.

Branch HEAD remains unsafe to ship. Immediate work is safety retirement, path
containment, and an early disposable adoption/handoff spike—not implementing
the existing A3 plan stack unchanged.
