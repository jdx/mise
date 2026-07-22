# brew-cask ownership, Homebrew metadata, and canonical direction

**Research date:** 2026-07-22  
**Verification audit:** 2026-07-23 — every upstream claim re-verified against
live GitHub data and `origin/main`; see [Verification audit](#verification-audit--2026-07-23)  
**Direction lock:** 2026-07-23 — formula-style cask identity is the **potential**
product direction (no upstream rejection proof); see
[Upstream rejection status](#upstream-rejection-status--generate-metadata) and
[Directions that resolve the product goals](#directions-that-resolve-the-product-goals)  
**Process:** always extend this file when research, direction, or branch
behavior changes — it is the durable decision record for the fork.  
**Repository:** `donbeave/mise`  
**Branch:** `fix/brew-cask-homebrew-metadata-receipt`  
**Scope:** research and decision record; never an upstream PR authorization

## Executive decision

**Potential product direction (bootstrap `brew-cask:` only): formula-style
coexistence** — Direction **A** below.

User expectation of `brew-cask:` under Homebrew’s prefix is that the install is
**observably a Homebrew cask** — without shelling out to Ruby `brew` for the
pour. That matches formulae today (`INSTALL_RECEIPT.json`) and was unfinished
for casks (`.mise-cask.toml` only on main).

| Layer | Contract |
|---|---|
| Pour engine | mise Rust direct install (no `brew install --cask`) |
| Mise ledger | `.mise-cask.toml` — status / paths / mise upgrade |
| Brew identity | `Caskroom/<token>/.metadata/` at pour time (cask analogue of formula receipt) |
| Preserve | Never destroy genuine Homebrew-authored `.metadata` (#11012) |
| Not in scope | Full brew replacement; perfect offline uninstall of every app layout |

**Ship** pour-time brew metadata for bootstrap casks. **Do not** mark a healthy
mise pour as package `Missing` only because brew’s tab was absent.

**Upstream status of this direction:** **not rejected** (never closed as WONTFIX;
never refused in jdx comments). **Not yet accepted** (no PR opened; #11012
shipped preserve-only for a different bug). Count as **potential**, not proven.

## Repository and operating policy

This work is restricted to the fork:

| Item | Value |
|---|---|
| Fork | <https://github.com/donbeave/mise> |
| Local clone | `/Users/donbeave/Projects/donbeave/mise` |
| Branch | `fix/brew-cask-homebrew-metadata-receipt` |
| Upstream | `jdx/mise`, read-only context |
| Fork remote | `fork` |

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

| Commit | Purpose |
|---|---|
| `bd2fe92bd` | Write Homebrew `.metadata` after a mise cask pour |
| `300c5c062` | Record initial root-cause research |
| `2712729ba` | Remove dangerous partial uninstall metadata |
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

| Token | Result before repair |
|---|---|
| `kimi` | Homebrew recognized it after an earlier synthetic metadata experiment |
| `codex` | Homebrew recognized it after real Homebrew adoption |
| `grok-build` | `Cask 'grok-build' is not installed` |
| `codexbar` | not installed according to Homebrew |
| `claude-code` | not installed according to Homebrew |
| `1password-cli` | not installed according to Homebrew |

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
"*This comment was generated by an AI coding assistant.*" They are posted from
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

| PR | Relevance |
|---|---|
| [#11197](https://github.com/jdx/mise/pull/11197) | Direct cask lifecycle and receipt correctness; architecturally important |
| [#11198](https://github.com/jdx/mise/pull/11198) | Direct completion ownership; architecturally important |
| [#11139](https://github.com/jdx/mise/pull/11139) | Release aggregation containing recent cask fixes |
| [#11172](https://github.com/jdx/mise/pull/11172) | Homebrew sync keyword match; unrelated to bootstrap cask ownership |

### Open discussions directly relevant to bootstrap Homebrew/casks

| Discussion | Subject | Direction/effect |
|---|---|---|
| [#10413](https://github.com/jdx/mise/discussions/10413) | Declarative package pruning | Preceded formula import/prune; jdx's entire reply is "sounds fine" — no cask rationale stated there |
| [#10582](https://github.com/jdx/mise/discussions/10582) | Broader cask types | Explicit mise-owned lifecycle; fail loudly |
| [#10598](https://github.com/jdx/mise/discussions/10598) | `1password-cli` binary | Led to direct binary support |
| [#10625](https://github.com/jdx/mise/discussions/10625) | Claude Code raw archive | Led to direct extraction support |
| [#10684](https://github.com/jdx/mise/discussions/10684) | Completions/manpages | Direct artifact coverage |
| [#10764](https://github.com/jdx/mise/discussions/10764) | VS Code suffixless ZIP | Direct archive sniffing |
| [#10765](https://github.com/jdx/mise/discussions/10765) | Font target expansion | Direct font support |
| [#10782](https://github.com/jdx/mise/discussions/10782) | Cask appdir options | Unresolved configuration surface |
| [#10917](https://github.com/jdx/mise/discussions/10917) | Localized casks/Ruby | Direct cask DSL execution |
| [#10968](https://github.com/jdx/mise/discussions/10968) | Intel macOS Homebrew | jdx explicitly declined Intel support |
| [#11007](https://github.com/jdx/mise/discussions/11007) | Deleted brew metadata | Preserve genuine Homebrew ownership |
| [#11058](https://github.com/jdx/mise/discussions/11058) | `__MACOSX` artifact twin | Direct artifact lookup fix |
| [#11156](https://github.com/jdx/mise/discussions/11156) | Yaak bundle case | Direct filesystem parity |
| [#11157](https://github.com/jdx/mise/discussions/11157) | VLC generated wrapper | Direct lifecycle parity |
| [#11168](https://github.com/jdx/mise/discussions/11168) | Docker `/usr/local` targets | Direct target-path parity |

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

| Claim | Verdict |
|---|---|
| #10326 promises full brew coexistence for formulae | Verified, verbatim ("sees mise's kegs as its own") |
| #10326 scope excludes casks | Verified, verbatim ("no casks") |
| #10383 makes no brew-compat promise for casks | Verified; zero `INSTALL_RECEIPT` occurrences in its diff |
| #11012 preserves, never creates, `.metadata` | Verified |
| jdx #10582 quotes ("mise-owned cask lifecycle runtime path", "intentionally not a `brew install --cask` fallback") | Verified, verbatim |
| jdx #11007 quote ("cannot be recovered by mise…") | Verified, verbatim |
| #11197/#11198 open, mise-owned receipts only | Verified; no `.metadata` writes in either diff |
| No upstream PR *implements* brew `.metadata` for mise-poured casks | Verified — zero PRs |
| No discussion body ever mentions generate as expected option | **Corrected 2026-07-23 third pass** — #11007 OP explicitly offered preserve **or generate**; no *follow-up PR* proposed implementation |
| Upstream docs coexistence section is formulae-only | Verified against `origin/main` `docs/bootstrap/packages/brew.md`; the cask coexistence text exists only on this branch |

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

| Branch component | Fit under Direction A (potential) |
|---|---|
| Pour-time `.metadata` write on fresh install | **Core of A** — formula #10326 analogue; exact current version |
| Repair of mise-owned pours missing brew tab (`.mise-cask.toml` proof) | **OK for A** — not “recover deleted brew-origin metadata”; only adopt earlier mise-only pours |
| Status flip (`Missing` when brew ledger absent) | **Removed on HEAD** — was wrong; mise status stays on payload/mise ledger |
| Empty `uninstall_artifacts` tab | Correct for brew API early-return; document offline/uninstall gaps |

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

| Looked for | Result |
|---|---|
| Discussion proposing generate as expected behavior | **#11007** (khoi) — preserve **or generate** |
| Merged response to #11007 | **#11012** — preserve/ignore only; does **not** generate |
| jdx text “do not generate” / “won’t support brew list for mise casks” | **None** |
| Open or closed PR that implements cask `.metadata` generation | **None** |
| Issue tracker WONTFIX for this | **None** (bootstrap cask reports live in Discussions) |
| Title trap | **#11107** “auto-updating cask metadata” = cask JSON `auto_updates` field, not Caskroom `.metadata` |

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

| ID | Goal |
|---|---|
| G1 | Install via `mise bootstrap` / `brew-cask:` (Rust pour, no `brew install --cask`) |
| G2 | User should not feel a different product than `brew install --cask` for **install identity** |
| G3 | `brew list --cask --versions TOKEN` works |
| G4 | `brew upgrade --cask TOKEN` installed-gate works (Codex-class tools) |
| G5 | mise still status/upgrade via bootstrap (`.mise-cask.toml`) |
| G6 | Never destroy genuine Homebrew `.metadata` (#11012) |
| G7 | Bootstrap-scoped — not full Homebrew replacement |

**Not required for “identity done”:** perfect offline uninstall of every app
layout, full zap parity, Intel macs, every artifact type.

## Directions that resolve the product goals

| ID | Direction | Full goal set? | jdx risk | Notes |
|---|---|---|---|---|
| **A** | Pour-time write brew `.metadata` + keep `.mise-cask.toml` | **Yes (identity)** | Med (open) | Only complete match for G1–G7 without shelling to brew |
| **B** | Single owner + disable tool brew self-update | No | Low (main today) | Ops workaround; G3/G4 stay broken |
| **C** | Install those casks with real `brew` only | Partial | Low | Abandons pure mise pour |
| **D** | Shell out to `brew install --cask` | No | **High reject** | Explicit non-goal (#10582) |
| **E** | Metadata only for simple binary casks | Partial | Med | Codex-class only; inconsistent ownership |
| **F** | Explicit opt-in adopt/handoff command | Partial | Med | Default still feels wrong |
| **G** | Fix only Codex / third-party tools | No | n/a | `brew` CLI still broken for humans |
| **H** | zerobrew-style private store | No | n/a | Not brew-visible; wrong reference |

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
