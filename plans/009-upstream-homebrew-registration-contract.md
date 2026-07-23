# Plan 009: Propose the smallest missing supported Homebrew handoff capability

**2026-07-23 supersession note**: Do not start with broad arbitrary external
registration. Homebrew already supports a full install/handoff through
`brew install --cask --adopt`. Run Plan 012 first. Continue this plan only for a
concrete gap that native install/adopt/force cannot safely cover, and ask for
the smallest machine-readable validation, dry-run, status, or staged-payload
capability that closes that proven gap.

> **Executor instructions**: This is a design and upstream-feasibility spike,
> not permission to publish, comment, open an issue, or open a PR. Complete the
> local proposal and maintainer review first. External contact requires explicit
> operator authorization and every GitHub message must say it was AI-generated.
>
> **Drift check (run first)**:
> `git diff --stat 866916893..HEAD -- src/system/packages/brew docs/dev plans`
> Re-read Homebrew's current cask loader, tab, installer, upgrade, migration,
> locking, and external-command code. Search for a supported registration API;
> do not assume the negative result from this planning pass remains current.

## Status

**DONE locally; no upstream contact.** Plan 012 selected unsupported/mise-only.
The smallest missing capability is a machine-readable, non-mutating
validation/dry-run bound to an atomic native handoff result—not broad
receipt-only registration. The local decision package is
`docs/dev/brew-cask-supported-handoff-gap.md`. External contact still requires
separate authorization.

- **Priority**: P2
- **Effort**: M-L
- **Risk**: HIGH
- **Depends on**: `plans/012-evaluate-homebrew-handoff.md`; proceed only when
  its decision table identifies a concrete missing supported capability
- **Category**: architecture / upstream / compatibility
- **Planned at**: commit `866916893`, 2026-07-23

## Why this matters

Homebrew's Caskroom metadata is private lifecycle authority. Mise can copy its
current shape, but cannot acquire Homebrew's lock, migration guarantees, or a
compatibility promise. Runtime drift checks gate future writes only; they do
not protect metadata already published and later consumed by a newer Homebrew.

The durable boundary is Homebrew owning its ledger and lifecycle. Existing
native install/adopt may already provide that boundary for some classes. Only a
proven remaining gap justifies an upstream proposal; Plans 002-008 remain
experimental/default-off and are not prerequisites for this plan.

## Current state

Current primary-source findings:

- `Cask::Tab` is documented private:
  <https://docs.brew.sh/rubydoc/Cask/Tab>.
- Caskroom APIs state that most methods are internal implementation details:
  <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/Library/Homebrew/cask/caskroom.rb#L7-L11>.
- External commands are separate executables, not a supported receipt-writing
  API:
  <https://github.com/Homebrew/brew/blob/c010c96b2366b30b73e3f0879dfc9b45ce79988c/docs/External-Commands.md#L25-L26>.
- Homebrew documents `brew install --cask --adopt` as a supported way to adopt
  an already-present app, but source shows it still runs the normal installer:
  <https://docs.brew.sh/Tips-and-Tricks#adopt-a-manually-installed-app>.
- This audit found no supported receipt-only API for registering an arbitrary
  externally poured cask. That absence is not proof such an API should exist.

## Commands you will need

| Purpose                  | Command                                                                                                              | Expected on success                     |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| Search local brew source | `rtk rg -n -e register -e "external.*cask" -e "Cask::Tab" -e metadata_main_container /opt/homebrew/Library/Homebrew` | every candidate reviewed                |
| Search mise coupling     | `rtk rg -n -e homebrew_cask -e "\.metadata" -e Caskroom src docs plans`                                              | private coupling inventory complete     |
| Docs build               | `rtk mise run docs:build`                                                                                            | exit 0 if proposal is placed under docs |
| Diff check               | `rtk git diff --check`                                                                                               | no output                               |

Use current official Homebrew sources and public documentation. If local source
is stale or absent, fetch the current upstream repository read-only and record
its commit.

## Scope

**In scope**:

- one concrete unsupported Plan 012 scenario;
- the smallest local, implementation-ready capability proposal that fixes it;
- Homebrew and mise ownership/transaction responsibilities for that operation;
- full registration/status/removal only if Plan 012 proves they are necessary;
- prototype/fixture only if it stays local and proves contract feasibility;
- explicit decision record if upstream support is unavailable.

**Out of scope**:

- writing private Homebrew metadata directly as the proposed public API;
- contacting upstream without explicit operator authorization;
- promising acceptance or a delivery date;
- making experimental interop default-on while the contract is absent;
- requiring Homebrew for normal mise-only cask install/status.
- a speculative multi-operation protocol without a failing scenario for each
  operation.

## Git workflow

- Branch: `advisor/009-homebrew-registration-contract`
- Commit: `docs(brew-cask): propose native handoff capability`
- Use `git commit -s`; include
  `Co-authored-by: Codex <codex@openai.com>`.
- Do not push, comment, open an issue, or open a PR unless operator asks.

## Steps

### Step 1: Import Plan 012's proven capability gap

Do not repeat speculative broad research. Start from Plan 012's failing matrix
rows, then trace current Homebrew public commands and code for:

- external install/register/adopt/import APIs;
- cask metadata validation/migration;
- locking around Caskroom install/upgrade/uninstall;
- installed JSON and tab creation;
- dependency and artifact serialization;
- transfer from an external manager to Homebrew, including `--adopt` and
  intentional native reinstall/`--force`.

Record exact commit links. Distinguish a callable public contract from private
Ruby methods, test helpers, and external-command discovery. A private method is
not “almost supported.”

**Verify**: every requested capability maps to one reproducible failing Plan
012 scenario; the proposal begins with supported today, private-only, and absent.

### Step 2: Specify only the smallest missing capability

Prefer extending supported install/adopt with machine-readable validation,
deterministic dry-run/status, or a narrowly bounded staged-payload handoff.
Propose external registration only if strict Rust-pour preservation remains a
confirmed requirement and Plan 012 proves native handoff cannot satisfy it.
If registration is still necessary, the following semantics are mandatory:

1. Input is a versioned manifest over stdin/file, never arbitrary Ruby.
2. Manifest names producer identity, token, opaque version, source/tap identity,
   selected platform/architecture, and a unique transaction ID.
3. It lists actual transformed artifact actions: artifact type, staged source,
   activated target, link/move/copy mode, digest/type, and uninstall behavior.
4. It records exact resolved formula/cask runtime dependencies, request origin,
   conflicts, and relevant config—not a live API pointer.
5. Homebrew validates every path and supported artifact class, rejects unknown
   fields/capabilities, acquires its own lock, and atomically writes/migrates its
   own ledger. Mise never chooses private filenames.
6. Response returns contract version, authoritative registration ID, content
   digest, lifecycle capabilities, and explicit rejection reasons.
7. Repeating the same transaction is idempotent; a different manifest for the
   same token requires compare-and-swap against the prior registration ID.

Do not require Homebrew to accept artifacts it cannot safely upgrade/uninstall.
Capability rejection leaves the valid mise payload mise-owned and invisible.

**Verify**: binary-only, dependency-bearing, unsupported app, unknown field,
foreign target, and repeated-request examples have deterministic outcomes.

### Step 3: Specify ownership for only the selected operation

State the selected operation's input, output, validation owner, mutation owner,
lock, linearization point, idempotency key, conflict result, and crash behavior.
If it extends native `install --cask --adopt`, Homebrew becomes sole owner and
mise becomes `Externalized`; do not add register/deregister concepts.

Only if Plan 012 proves receipt-only registration is required, define the
minimum coherent set: read-only `validate`, atomic `register`, authoritative
`status`, compare-and-swap `deregister`, and explicit lifecycle `adopt`. Explain
why each operation has its own failing scenario. Homebrew owns ledger locking
and migration. Mise never claims cross-manager atomicity without that handshake.

**Verify**: sequence diagrams cover success, rejection, retry, crash immediately
before/after the operation's linearization point, and two concurrent callers.

### Step 4: Define compatibility and security boundaries

Require only capabilities relevant to the selected operation, including:

- schema version plus capability negotiation;
- path canonicalization and prefix/target allowlists;
- no symlink traversal during validation;
- digest/type verification before lifecycle authority is granted;
- producer identity as provenance, not trust by itself;
- no shell/Ruby execution from manifest fields;
- size/count limits and actionable structured errors;
- Homebrew-owned migrations across future releases;
- a documented support/deprecation policy for accepted schema versions.

The API must not transform “I can see files” into authority to delete unrelated
files. Zap and flight blocks remain unsupported unless separately represented
and validated.

**Verify**: threat fixtures cover traversal, target substitution, symlink race,
replay, stale compare-and-swap, oversized manifest, and unknown artifact.

### Step 5: Map the selected capability back to mise

Design a thin adapter:

- mise install works without Homebrew and remains `MiseOnly`;
- when Homebrew is present and explicit handoff is requested, mise supplies
  only Plan 013's validated completed-action facts required by the operation;
- normal apply never hands off or registers implicitly and never reconstructs
  legacy state from current API;
- successful Homebrew takeover becomes `Externalized`; mise refuses mutation;
- status/recovery uses public results only when the selected capability exposes
  them, otherwise it preserves state and fails closed;
- private `.metadata` paths never become part of the adapter contract.

If the public contract requires Homebrew to be installed, only interop requires
it. G1's Rust pour and ordinary mise-only behavior remain intact.

**Verify**: map every affected Plan 001/012 state and phase to public results;
all unaffected states have explicit no-op/fail-closed behavior.

### Step 6: Produce a decision package; contact only when authorized

Create a concise local proposal containing problem statement, current failure
evidence, contract, security model, sequence diagrams, examples, and open
questions. Obtain local maintainer review.

Then stop. If the operator explicitly authorizes upstream contact, follow the
chosen contribution channel, minimize the proposal to upstream conventions,
and label every comment/PR as AI-generated. Record upstream feedback as
requirements; do not reinterpret rejection as partial approval.

Decision outcomes:

- **Accepted/available**: create a new implementation plan using the supported
  API. Keep private metadata adapter disabled by default during migration.
- **Needs revision**: update proposal and tests; no production promise.
- **Declined/no contract**: document that robust G2-G4 are unavailable. Keep
  Plans 002-008 experimental/default-off with explicit supported-version
  bounds, or remove private publication entirely.

**Verify**: one signed-off local decision names the outcome and product
boundary; no external side effect occurred without authorization.

## Test plan

- Fixture validation for the selected operation's supported and hostile input.
- Idempotency/conflict model tests and crash/retry around its linearization.
- The exact Plan 012 failure becomes a passing contract example.
- One unsupported artifact/dependency case remains fail-closed.
- If registration is selected, additionally test schema negotiation,
  compare-and-swap, status, deregister, and handoff.
- No private path assertions in the final adapter contract.

## Done criteria

- [ ] Current upstream has been searched and evidence pinned to a commit.
- [ ] Every requested capability maps to a reproducible Plan 012 failure.
- [ ] The proposal contains no operation beyond the minimum coherent fix.
- [ ] Ownership, locking, linearization, retry, migration, and security are
      explicit for every selected operation.
- [ ] Every original G1-G7 goal has an honest supported/conditional/impossible
      result under both accepted and declined outcomes.
- [ ] Local maintainer decision is recorded.
- [ ] No upstream contact occurred without explicit authorization.
- [ ] `rtk git diff --check` emits no output.

## STOP conditions

Stop and report if:

- current Homebrew already exposes a supported equivalent for the exact Plan
  012 gap—replace this design with an integration plan;
- the proposal requires direct private metadata writes;
- lifecycle safety still depends on current live API recovery;
- cross-manager races lack an owner/lock/compare-and-swap rule;
- a requested external action lacks explicit operator authorization;
- upstream declines the contract and product docs still promise robust G2-G4.

## Maintenance notes

- Keep the public manifest semantic. Private Homebrew filenames and Ruby class
  layouts must not leak into the contract.
- Any registration, if still justified, means lifecycle authority, not identity
  decoration.
- “Homebrew not installed” and “Homebrew interop unavailable” are valid states;
  neither should break mise payload management.
