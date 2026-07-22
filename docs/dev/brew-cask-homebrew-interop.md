# brew-cask ↔ real Homebrew interop (investigation + fix)

**Branch:** `fix/brew-cask-homebrew-metadata-receipt`  
**Fork:** `donbeave/mise` (no upstream PR yet)  
**Date:** 2026-07-22  
**Reproducer host:** macOS arm64, Homebrew 6.x, mise 2026.7.11 (pre-fix)

## Summary

| Layer | Formulae (`brew:`) | Casks (`brew-cask:`) before this branch | After this branch |
|-------|--------------------|----------------------------------------|-------------------|
| Artifact pour | Cellar + links | Caskroom + `/Applications` or `bin` links | same |
| Mise receipt | n/a (uses brew tab) | `Caskroom/<token>/<ver>/.mise-cask.toml` | still written |
| Brew receipt | `Cellar/.../INSTALL_RECEIPT.json` | **missing** | `.metadata/INSTALL_RECEIPT.json` + installed caskfile |
| `brew list --versions` | works | **fails** ("not installed") | works |
| `brew upgrade` | works | **fails** | works (when outdated) |

**Root cause (one line):** cask pour was built as a **standalone installer that
never shells out to brew**, with a **mise-private receipt** (`.mise-cask.toml`),
while formula pour from day one wrote a **brew-compatible tab**. Docs claimed
Homebrew coexistence for **formulae only**; casks never got that parity. Later
PRs **learned** about `.metadata` only to **preserve** brew-written trees, not
to **emit** them.

## Why mise itself created this issue (research)

This is not a random regression. It follows from deliberate design choices
and incomplete coexistence parity. Timeline from `jdx/mise` history:

### 1. Formulae: “brew without brew” + coexistence from day one

- **PR / commit:** [#10326](https://github.com/jdx/mise/pull/10326)
  `feat(system): declarative system packages (apt, dnf, pacman, and brew without brew)`
  (`173403d57`, 2026-06-11)
- **Design slogan:** install homebrew/core bottles **without requiring Homebrew
  to be installed**, **never shell out to `brew`**.
- **Coexistence (docs from that PR):** still write
  brew-compatible `INSTALL_RECEIPT.json` into each keg so a later-installed real
  Homebrew treats mise pours as its own (`brew list` / `upgrade` / `uninstall`).
- **Implementation:** `src/system/packages/brew/pour.rs` → `write_receipt` with
  explicit comment:
  > brew-compatible INSTALL_RECEIPT.json so a later-installed real Homebrew
  > adopts these kegs
- **Casks:** explicitly **out of scope** (“Formulae only. Casks (GUI apps) …
  are not implemented”).

So formula interop was a **hard requirement**. Cask interop did not exist yet.

### 2. Casks: same “no brew CLI” model, different receipt story

- **PR / commit:** [#10383](https://github.com/jdx/mise/pull/10383)
  `feat(bootstrap): support brew taps and casks directly`
  (`28c095dd5`, 2026-06-12/13)
- **What landed:** new `BrewCaskManager` in `src/system/packages/brew/cask.rs`.
  - Fetch `api/cask/<token>.json` (or tap API), download, sha256, extract.
  - Stage under `Caskroom/<token>/<version>/`.
  - Link apps to `/Applications` (later: binaries, pkg, fonts, hooks).
  - **Status / “already installed”** = local Caskroom dirs + **`.mise-cask.toml`**
    (and/or presence of app targets). **Not** Homebrew’s `.metadata`.
- **Receipt invented for mise only:**
  ```rust
  // original write_receipt (28c095dd5)
  crate::file::write(caskroom.join(".mise-cask.toml"), body)?;
  ```
  Private TOML: `{ version, apps }` (later binaries/fonts/pkg_ids). Homebrew
  **never** reads `.mise-cask.toml`.
- **Explicit non-goals in PR + docs:**
  - “without requiring Homebrew to be installed”
  - “fails with a clear unsupported artifact error **instead of delegating
    to Homebrew**”
  - “Cask artifact coverage is **intentionally narrow**”
  - Validation even grepped that install paths do **not** call `brew` CLI.
- **Coexistence section of brew docs was left formula-centric.** After #10383
  it still said only kegs get `INSTALL_RECEIPT.json` and “look like” brew’s own.
  **No sentence promised the same for casks.**

**Architectural consequence:** mise and brew can share the **prefix layout**
(`Caskroom/`, `/Applications`, `bin/` links) while speaking **different
ledger formats**. That is the structural bug class.

```text
mise success criterion:  Caskroom/<token>/<ver> exists + apps/binaries on disk
                         + optional .mise-cask.toml
brew success criterion:  .metadata/<ver>/<ts>/Casks/<token>.{json|rb}
                         (+ INSTALL_RECEIPT.json tab)
```

When a **third party** (Codex) assumes “binary under `$HOMEBREW_PREFIX` ⇒ brew
owns it ⇒ `brew upgrade --cask`”, the ledger mismatch becomes a user-visible
failure even though mise install “worked.”

### 3. mise later *discovered* `.metadata` — but only as something to not break

- **Discussion:** [#11007](https://github.com/jdx/mise/discussions/11007)
  (referenced by PR body)
- **PR / commit:** [#11012](https://github.com/jdx/mise/pull/11012)
  `fix(bootstrap): preserve Homebrew cask metadata`
  (`d9747b57f`, 2026-07-15)

**What went wrong before #11012:**

1. Real Homebrew cask has both `<version>/` and `.metadata/` under
   `Caskroom/<token>/`.
2. `installed_version()` treated **every** subdirectory as a version → saw
   two “versions” (e.g. `2.0.0` + `.metadata`) → “multiple versions” / reinstall.
3. `remove_stale_versions()` deleted **all** dirs except current version —
   including **`.metadata`** → brew could no longer inspect/upgrade that cask.

**What #11012 fixed:**

- Skip `.metadata` and `.mise-tmp-*` when detecting versions.
- Never delete `.metadata` in stale cleanup.
- Tests named `installed_version_ignores_homebrew_metadata` and
  `remove_stale_versions_keeps_current_version_and_homebrew_metadata`.

**What #11012 did *not* fix:**

- It **never wrote** `.metadata` for **mise-originated** pours.
- Direction of interop: **brew → mise** (don’t destroy brew’s ledger), not
  **mise → brew** (emit brew’s ledger).

That is the smoking gun that maintainers **knew** the brew ledger path, yet
coexistence remained one-way.

### 4. Why the incomplete parity was “rational” at the time

| Pressure | Effect on design |
|----------|------------------|
| “No Homebrew required” | Cannot depend on `brew install --cask` Ruby stack for install |
| “Never shell out to brew” (validated in #10383) | Must reimplement pour; easy to invent private receipt |
| Narrow artifact MVP (app only, then binary/pkg…) | Focus on files on disk, not full `Cask::Tab` / uninstall artifacts |
| Formulae already hard for interop | Keg tab was mandatory; cask tab deferred |
| Status checks use filesystem | mise does not need brew’s `installed?` to manage its own packages |
| Import/prune still formulae-only (docs limitations) | Cask lifecycle not treated as full brew citizen |

None of that makes the Codex failure “user error.” It means **product surface
shared with brew** (prefix + binary path) without **ledger parity**.

### 5. Causal chain for our failure

```text
#10326  formula pour + INSTALL_RECEIPT  (brew list/upgrade OK)
   │
#10383  brew-cask pour + .mise-cask.toml only
   │      docs coexistence still formula-only
   │      installed? for mise = Caskroom version dir + apps
   │
#11012  learn .metadata exists → preserve, do not emit
   │
user    mise bootstrap brew-cask:codex
   │      → /opt/homebrew/bin/codex + Caskroom + .mise-cask.toml
   │      → no .metadata → brew: "not installed"
   │
Codex   brew upgrade --cask codex  → hard fail on startup
```

### 6. Issues inside mise (checklist)

| # | Issue | Evidence |
|---|--------|----------|
| A | **Missing brew cask tab / installed caskfile on pour** | `write_receipt` → `.mise-cask.toml` only since #10383 |
| B | **Docs over-promise shared prefix, under-specify cask ledger** | Coexistence section formula-only through 2026.7.11 |
| C | **One-way interop after #11012** | Preserve `.metadata` if brew wrote it; mise pours still orphans |
| D | **`installed_version` / cleanup historically hostile to `.metadata`** | Fixed in #11012 for brew-adopted casks only |
| E | **No e2e that runs real `brew list --cask --versions` after mise pour** | macOS e2e checks Caskroom/app dirs, not brew CLI |
| F | **Cask import/prune not implemented** | Limitations in brew.md — incomplete brew citizen lifecycle |
| G | **Artifact coverage gaps** (separate from receipt) | pkg/postflight/completions; #11164 etc. |

**Primary fix for A–C:** emit Homebrew `.metadata` on every successful cask pour
(this branch). **Secondary:** e2e with `brew` present asserting
`brew list --cask --versions <token>`; doc coexistence for casks; optional
backfill command for existing mise-orphans.

## Real-world failure (Codex / essential-mac)

1. User declares `"brew-cask:codex" = "latest"` in `[bootstrap.packages]`.
2. `mise bootstrap` pours binary to
   `/opt/homebrew/Caskroom/codex/<ver>/` and links `/opt/homebrew/bin/codex`.
3. Writes only `.mise-cask.toml` (version + artifact paths).
4. On startup, Codex detects a Homebrew-prefix binary and runs:
   ```sh
   brew upgrade --cask codex
   ```
5. Homebrew:
   ```text
   Error: Cask 'codex' is not installed.
   ```

Install **succeeded**. Self-update via brew **failed** because of missing
metadata.

Same class of orphans observed on a developer machine (2026-07-22):

| Token | `.mise-cask.toml` | `.metadata/INSTALL_RECEIPT.json` | `brew list --cask --versions` |
|-------|-------------------|----------------------------------|-------------------------------|
| kimi | yes | no (before experiment) | Error: not installed |
| grok-build | yes | no | Error: not installed |
| codexbar | yes | no | Error: not installed |
| claude-code | yes | no | Error: not installed |
| 1password-cli | yes | no | Error: not installed |
| codex | (after real `brew install --cask --force`) | yes | `codex 0.145.0` |

Note: bare `brew list --cask` may still **print** token names when Caskroom
directories exist, even without metadata. Always use
`brew list --cask --versions TOKEN` to detect a real install. Homebrew also
exposes these as `Caskroom.corrupt_cask_dirs`.

## How Homebrew decides "installed"

From `/opt/homebrew/Library/Homebrew/cask/`:

```ruby
# cask.rb
def installed?
  installed_caskfile&.exist? || false
end

# caskroom.rb
def self.cask_with_metadata?(cask_path)
  cask_path.glob(".metadata/*/*/Casks/*.{rb,json}").any?
end
```

Required layout:

```text
$HOMEBREW_PREFIX/Caskroom/<token>/
  <version>/                    # staged artifacts (mise already does this)
  .metadata/
    INSTALL_RECEIPT.json        # Cask::Tab (version in source.version)
    config.json                 # optional but brew writes it
    <version>/<timestamp>/Casks/<token>.json   # or .rb — existence matters
```

Timestamp format: `Metadata::TIMESTAMP_FORMAT` = `%Y%m%d%H%M%S.%L`.

### Minimal experiment (kimi, already mise-poured 3.1.2)

Created only:

- `.metadata/INSTALL_RECEIPT.json` with `source.version = "3.1.2"`
- `.metadata/3.1.2/<ts>/Casks/kimi.json` → `{}`
- `.metadata/config.json` minimal

Results:

```text
$ brew list --cask --versions kimi
kimi 3.1.2

$ brew info --cask kimi
Installed (on request)
...

$ brew upgrade --cask --dry-run kimi
==> Would upgrade 1 outdated package
kimi 3.1.2 -> 3.1.3
```

Empty `{}` cask JSON matches what current Homebrew writes for several
API-installed casks (bartender, vlc, codex).

### Sample brew tab shape (codex 0.145.0, real brew)

Key fields:

- `homebrew_version`
- `loaded_from_api` / `loaded_from_internal_api`
- `installed_on_request`
- `time`
- `runtime_dependencies` (object; may be empty `{}`)
- `source.tap`, `source.version`, `source.path`, `source.tap_git_head`
- `arch`
- `uninstall_artifacts` (array of artifact hashes)
- `built_on`

## Formula parity (already correct)

`src/system/packages/brew/pour.rs` → `write_receipt` writes
`INSTALL_RECEIPT.json` into the keg and documents:

> brew-compatible INSTALL_RECEIPT.json so a later-installed real Homebrew
> adopts these kegs (brew list/upgrade/uninstall all work).

Casks needed the same contract under `.metadata/`.

## Fix (this branch)

In `src/system/packages/brew/cask.rs` after a successful pour:

1. Keep writing mise `.mise-cask.toml` in the versioned caskroom (unchanged).
2. Call `write_homebrew_cask_metadata(token_dir, cask, artifacts)`:
   - Write `.metadata/<version>/<timestamp>/Casks/<token>.json` (`{}`)
   - Write `.metadata/INSTALL_RECEIPT.json` with mise-marked
     `homebrew_version: "5.1.15 (mise)"`, `source.version`,
     **`uninstall_artifacts: []` (empty on purpose)**
   - Write `.metadata/config.json` if missing
   - Replace prior versioned metadata dirs so `installed_version` matches
   - UTC timestamps (match brew `Time.now.utc`)

Docs: `docs/bootstrap/packages/brew.md` coexistence section updated (with
caveats).

### Verification (multi-wave, 2026-07-22) — skeptical review

Three independent analyses + local Homebrew Ruby reading:

| Claim | Verdict |
|-------|---------|
| Approach (emit `.metadata`) correct | **YES** — formula receipt parity; only way to fix brew `installed?` without shelling out |
| Empty `{}` caskfile OK | **YES** — brew itself writes `{}` when no `only_path`; disk samples match |
| Path enough for list/info/upgrade *gate* | **YES** — Ruby `installed?` / `cask_installed_version` + kimi dry-run |
| First implementation's partial uninstall list | **WRONG** — non-empty partial list **blocks** API recovery (`resolve_installed_artifacts` early return). Fixed: empty `[]` |
| Fake `"pkg": [source]` uninstall entry | **WRONG** — removed with empty list; real uninstall is pkgutil stanza |
| Full `brew uninstall` / clean upgrade for **app** casks | **NOT guaranteed** — mise `ditto` copies apps; brew often move+symlink; residual risk |
| Dual ownership | **Intentional** — same class as formula coexistence; brew upgrade can replace mise pour |
| Shell out to brew as default | **Reject** — breaks “brew without brew” |
| Document-only | **Reject** as steady state for shared prefix |

**Ship verdict:** PARTIALLY COMPLETE but approach correct.

- **Ship for:** list/info/upgrade-not-installed/Codex-class binary self-update
- **Do not claim:** full uninstall/upgrade lifecycle for every app/pkg cask
- **confidence:** ~85 approach; ~70 residual lifecycle safety after empty-tab fix

## Other brew-cask gaps (out of scope for this branch)

Tracked for later / already partially fixed elsewhere:

| Gap | Symptom | Notes |
|-----|---------|-------|
| pkg / complex postflight | install fails or incomplete | orbstack, zoom, etc. |
| Generated completions | not installed | codex cask declares completion artifact |
| auto_updates | was hard-fail; OK since 2026.7.11 | jdx/mise#11084 / #11107 |
| Yaak app casing / VLC preflight wrapper | pour fail | jdx/mise#11164 (may predate release) |
| TablePlus large DMG | mise HTTP decoder timeout | `brew fetch --cask` works |
| Intel macOS brew prefix | formulae skipped | discussion #10968 |

These are separate from receipt interop.

## essential-mac workaround (until this ships)

While running mise without this fix:

- Keep `"brew-cask:codex"` in mise (install works).
- `essential-mac setup-ai` sets `check_for_update_on_startup = false` so Codex
  does not call `brew upgrade --cask` on startup; updates via mise bootstrap.
- Optional: `brew install --cask --force <token>` adopts an existing mise pour
  and writes real metadata (verified for codex).

After this fix is in a released mise, the setup-ai guard becomes optional.

## Verification checklist

```sh
# unit
cargo test -p mise write_homebrew_cask_metadata
cargo test -p mise homebrew_uninstall_artifacts
cargo test -p mise --lib system::packages::brew::cask

# manual (after building this branch)
# 1) remove brew metadata from a mise-only cask, reinstall via mise brew-cask
# 2) brew list --cask --versions <token>   # must print version
# 3) brew upgrade --cask --dry-run <token> # must not say "not installed"
# 4) codex (with check_for_update on) should not print brew upgrade failure
```

## Upstream PR (not opened yet)

When opening against `jdx/mise`:

- Title: `fix(brew-cask): write Homebrew .metadata so brew list/upgrade work`
- Point to this doc + Codex repro
- Emphasize formula/cask parity for coexistence section of brew docs
- No behavior change for hosts without Homebrew installed (metadata is files only)
