# brew <Badge type="warning" text="experimental" />

Homebrew formulae on macOS — **without requiring Homebrew to be installed**.

```toml
[system.packages]
brew = ["postgresql@17", "ffmpeg", "imagemagick"]
```

mise installs [homebrew/core](https://formulae.brew.sh) formulae directly
into `/opt/homebrew`: it fetches metadata from the formulae.brew.sh API,
resolves the runtime dependency closure, downloads prebuilt bottles from
ghcr.io (verifying sha256 checksums), and performs the same relocation,
code-signing, and linking work `brew` does when pouring a bottle.

This exists because shared-library packages — postgres, ffmpeg, imagemagick,
php — fundamentally can't be served by mise's per-project backends like
`aqua:` or `github:`: their bottles are built against fixed install paths and
a shared dependency tree. Installing them at Homebrew's canonical prefix is
what makes them work.

::: info
arm64 (Apple Silicon) only. Intel macs are not supported — the `brew`
manager reports itself unavailable there.
:::

## If Homebrew is already installed

mise never shares a poured prefix with a real Homebrew. When brew is detected
at `/opt/homebrew`, `mise system install` simply delegates to
`brew install` — same declarative config, brew does the work.

The reverse direction is also safe: mise writes brew-compatible
`INSTALL_RECEIPT.json` files into every keg it pours, so if you later install
the real Homebrew, it adopts mise's kegs seamlessly — `brew list`,
`brew upgrade`, and `brew uninstall` all work on them.

## The prefix

If `/opt/homebrew` doesn't exist, mise creates it with the standard layout —
the only time the brew manager uses sudo, mirroring what Homebrew's own
installer does (`mkdir` + `chown` to your user). After that, installs are
plain file operations as your user; nothing runs as root.

## How pouring works

For each formula in the dependency closure (dependencies first):

1. **Fetch** the bottle for your macOS version from ghcr.io and verify its
   sha256 against the API metadata.
2. **Extract** into a temporary directory inside the Cellar (incomplete
   pours are never visible as installed packages).
3. **Relocate**: bottles embed placeholder paths like `@@HOMEBREW_PREFIX@@`.
   mise rewrites them to real paths — plain replacement in text files,
   in-place and load-command rewriting in Mach-O binaries (growing load
   commands into header padding when needed, exactly like brew's ruby-macho
   does).
4. **Re-sign**: any modified binary is ad-hoc re-signed with
   `codesign` — required on arm64, where the kernel kills binaries whose
   signature doesn't match.
5. **Receipt**: a brew-compatible `INSTALL_RECEIPT.json` is written.
6. **Link**: `/opt/homebrew/opt/<name>` is created and the keg's `bin`,
   `lib`, `include`, `share`, etc. are symlinked into the prefix —
   [keg-only](https://docs.brew.sh/FAQ#what-does-keg-only-mean) formulae get
   the `opt` link but are not linked into the prefix, same as brew.

mise records what it installed in its own ledger
(`~/.local/state/mise/system/brew.json`) and never overwrites files in the
prefix that it (or brew) didn't create — link conflicts fail with a list of
the offending files rather than clobbering them.

## Limitations

- **Formulae only.** Casks (GUI apps) and `brew services` are not
  implemented.
- **No taps.** Third-party taps are Ruby code that requires Homebrew to
  evaluate; only homebrew/core is supported. If you need a tap, install
  Homebrew and mise will delegate to it.
- **No source builds.** Formulae without a bottle for your macOS version
  fail with a clear error.
- **Use canonical formula names.** `postgresql@17` is a formula name, not a
  mise version pin — the API's current stable version decides what gets
  installed. Aliases (`postgres`) install correctly but `mise system status`
  can't track them; mise warns and tells you the canonical name.
- `PATH` is up to you: `/opt/homebrew/bin` must be on `PATH` to use linked
  binaries, just like with Homebrew itself.
