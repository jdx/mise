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

**Root cause:** mise's brew-cask shim intentionally does not shell out to
`brew install --cask`. Formula pour already writes a brew-compatible tab.
Cask pour only wrote mise-private `.mise-cask.toml`. Homebrew never reads that
file.

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
     `homebrew_version: "5.1.15 (mise)"`, `source.version`, basic
     `uninstall_artifacts` from app/binary/font/pkg lists
   - Write `.metadata/config.json` if missing
   - Replace prior versioned metadata dirs so `installed_version` matches

Docs: `docs/bootstrap/packages/brew.md` coexistence section updated.

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
