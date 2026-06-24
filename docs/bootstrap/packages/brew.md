# brew <Badge type="warning" text="experimental" />

Homebrew formulae and casks — **without requiring Homebrew to be installed**.

```toml
[bootstrap.packages]
"brew:postgresql@17" = "latest"
"brew:ffmpeg" = "latest"
"brew:imagemagick" = "latest"
"brew-cask:firefox" = "latest"
```

mise installs [homebrew/core](https://formulae.brew.sh) formulae directly
into the canonical Homebrew prefix — `/opt/homebrew` on arm64 macOS,
`/home/linuxbrew/.linuxbrew` on Linux. It fetches metadata from the
formulae.brew.sh API, resolves the runtime dependency closure, downloads
prebuilt bottles from ghcr.io (verifying sha256 checksums), and performs the
same relocation, code-signing, and linking work `brew` does when pouring a
bottle. Formulae without a usable bottle are built from source, also without
Homebrew (see [Source formulae](#source-formulae)). mise never shells out to
`brew` for homebrew/core formulae.

Third-party taps are supported directly when the tap publishes Homebrew API
metadata (`api/formula/<name>.json` or `api/cask/<token>.json`). Use the same
fully-qualified name you would pass to Homebrew:

```toml
[bootstrap.packages]
"brew:railwaycat/emacsmacport/emacs-mac" = "latest"
"brew-cask:owner/tap/app" = "latest"
```

For taps whose GitHub URL cannot be inferred, add a tap source. This mirrors
`[plugins]`: the key is the tap name and the value is the GitHub git URL.

```toml
[bootstrap.brew.taps]
"acme/tools" = "https://github.com/acme/homebrew-tools.git"

[bootstrap.packages]
"brew:acme/tools/widget" = "latest"
"brew-cask:acme/tools/widget-app" = "latest"
```

`mise bootstrap packages brew tap` and `mise bootstrap packages brew untap`
manage `[bootstrap.brew.taps]` in `mise.toml`; they do not mutate a Homebrew
installation. Non-GitHub taps are not currently supported because mise needs
direct raw access to the generated API metadata.

```sh
mise bootstrap packages brew tap railwaycat/emacsmacport
mise bootstrap packages brew tap acme/tools https://github.com/acme/homebrew-tools.git
mise bootstrap packages brew untap acme/tools
```

## Casks

Casks use the `brew-cask:` manager. mise fetches cask metadata directly from
the Homebrew cask API (or from tap API metadata), downloads the artifact,
verifies its sha256 when the cask provides one, extracts the archive, and
installs app bundles into `/Applications` while recording the version under
`<prefix>/Caskroom`.

```toml
[bootstrap.packages]
"brew-cask:firefox" = "latest"
"brew-cask:homebrew/cask/visual-studio-code" = "latest"
```

`brew-cask` currently supports app-bundle casks (`app` artifacts), binary casks
(`binary` artifacts), and simple macOS installer packages (`pkg` artifacts)
from dmg and common archive formats. Binary artifacts are staged in the Caskroom
and linked into the Homebrew prefix, usually under `<prefix>/bin`. Package
installers run through mise's normal system-package sudo path, so non-interactive
runs never hang waiting for a password. Pkg casks must include `pkgutil` receipt
IDs in their `uninstall` or `zap` metadata so mise can verify installed state
after the installer writes files outside the Caskroom. Casks that require custom
installer choices, services, or other cask artifact types fail with a clear
unsupported artifact error instead of delegating to Homebrew. Lifecycle metadata
such as `preflight` and `postflight` is not executed, and generated shell
completions are not installed.

This exists because shared-library packages — postgres, ffmpeg, imagemagick,
php — fundamentally can't be served by mise's per-project backends like
`aqua:` or `github:`: their bottles are built against fixed install paths and
a shared dependency tree. Installing them at Homebrew's canonical prefix is
what makes them work.

## Supported platforms

| Platform                    | Prefix                       |
| --------------------------- | ---------------------------- |
| macOS arm64 (Apple Silicon) | `/opt/homebrew`              |
| Linux x86_64                | `/home/linuxbrew/.linuxbrew` |
| Linux arm64                 | `/home/linuxbrew/.linuxbrew` |

Intel macs are not supported — the `brew` manager reports itself unavailable
there. On Linux, formulae without a bottle for your architecture (arm64
Linux bottles exist for most but not all of homebrew/core) are built from
source instead.

## The prefix

If the prefix doesn't exist, mise creates it with the standard layout — the
only time the brew manager uses sudo, mirroring what Homebrew's own installer
does (`mkdir` + `chown` to your user). After that, installs are plain file
operations as your user; nothing runs as root.

## Coexistence with a real Homebrew

mise pours bottles into the Cellar exactly the way brew does and writes
brew-compatible `INSTALL_RECEIPT.json` files into every keg. To a real
Homebrew installation, mise-poured kegs look like its own: `brew list`,
`brew upgrade`, and `brew uninstall` all work on them. Conversely, mise's
status checks read the Cellar directly, so formulae installed by brew count
as installed.

mise tracks what it installed in its own ledger
(`~/.local/state/mise/system/brew.json`) and never overwrites files in the
prefix that it (or brew) didn't create — link conflicts fail with a list of
the offending files rather than clobbering them.

## Importing and pruning

`mise bootstrap packages import --manager brew` snapshots installed Homebrew
formulae into `[bootstrap.packages]`, similar in spirit to
[`brew bundle dump`](https://docs.brew.sh/Brew-Bundle-and-Brewfile). It reads
the active `opt` links in the Homebrew prefix and writes entries like:

```toml
[bootstrap.packages]
"brew:ffmpeg" = "latest"
"brew:postgresql@17" = "latest"
```

By default, import records only formulae whose active keg receipt says they
were installed on request. Pass `--all` to include dependency formulae too.
Tapped formulae are written with fully-qualified names, and mise adds inferred
`[bootstrap.brew.taps]` entries when it can derive the conventional GitHub tap
URL:

```toml
[bootstrap.brew.taps]
"acme/tools" = "https://github.com/acme/homebrew-tools.git"

[bootstrap.packages]
"brew:acme/tools/widget" = "latest"
```

Import also adopts the imported roots and their resolved dependency closure
into mise's brew ledger. That ownership boundary is what makes pruning safe:
`mise bootstrap packages prune --manager brew` removes only formulae that mise
installed or adopted and that are no longer needed by the merged
`[bootstrap.packages]` config. Formulae installed by a real Homebrew are not
pruned until you import/adopt them.

Prune removes the active keg, the `opt` link, and prefix symlinks pointing into
that keg. It also forgets stale ledger entries for kegs that no longer exist.
Use `--dry-run` to preview and `--yes` to skip the confirmation prompt.

This command is mise's declarative cleanup for bootstrap packages, similar to
[`brew bundle cleanup`](https://docs.brew.sh/Manpage). It is not upstream
`brew prune`, which Homebrew removed in favor of cleanup commands.

## How pouring works

For each formula in the dependency closure (dependencies first):

1. **Fetch** the bottle for your platform from ghcr.io and verify its sha256
   against the API metadata.
2. **Extract** into a temporary directory inside the Cellar (incomplete
   pours are never visible as installed packages).
3. **Relocate**: bottles embed placeholder paths like `@@HOMEBREW_PREFIX@@`.
   mise rewrites them to real paths — plain replacement in text files,
   in-place and load-command rewriting in Mach-O binaries (growing load
   commands into header padding when needed, exactly like brew's ruby-macho
   does). On Linux, the ELF interpreter and rpath are patched the way
   brew's PatchELF gem does it: strings that no longer fit are moved into a
   new segment appended to the binary, and the interpreter is pointed at
   `<prefix>/lib/ld.so` (a symlink mise maintains to the system's dynamic
   loader, or to a brewed glibc when one is installed).
4. **Re-sign** (macOS): any modified binary is ad-hoc re-signed with
   `codesign` — required on arm64, where the kernel kills binaries whose
   signature doesn't match.
5. **Receipt**: a brew-compatible `INSTALL_RECEIPT.json` is written.
6. **Link**: `<prefix>/opt/<name>` is created and the keg's `bin`, `lib`,
   `include`, `share`, etc. are symlinked into the prefix —
   [keg-only](https://docs.brew.sh/FAQ#what-does-keg-only-mean) formulae get
   the `opt` link but are not linked into the prefix, same as brew.

## Source formulae

A few formulae have no bottle at all (source-only formulae), and some have
bottles for other platforms but not yours. mise builds those from source —
still without Homebrew:

1. **Ruby** — a formula is Ruby code, so mise provisions a mise-managed
   ruby through its normal tool machinery (precompiled, fast; respects your
   configured ruby if you have one).
2. **Formula** — the formula's `.rb` is downloaded from homebrew/core,
   pinned to the exact commit the API metadata was generated from and
   verified against the API's sha256 for it.
3. **Source** — the stable source archive is downloaded and verified
   against the API's sha256.
4. **Build deps** — the formula's build dependencies (cmake, pkgconf, ...)
   are added to the install closure and poured as regular bottles first.
5. **Build** — mise evaluates the formula with its own Formula-DSL shim and
   runs `def install` against the canonical prefix, with `PATH`,
   `PKG_CONFIG_PATH`, and compiler flags pointing at the dependency kegs.
   The keg gets the same brew-compatible receipt as a poured bottle, with
   `poured_from_bottle: false` — exactly how brew marks its own source
   builds.

The shim implements the commonly-used subset of the formula DSL
(configure/cmake/meson-style builds, resources, patches, the standard path
and environment helpers). Formulae that use parts of the DSL the shim
doesn't cover — language-specific helpers like `virtualenv_install_with_resources`,
VCS downloads, and similar — fail with a clear `formula uses ...` error
rather than miscompiling silently.

Source builds need a working toolchain (Xcode Command Line Tools on macOS,
gcc/make on Linux), exactly as they would under plain Homebrew.

## Upgrades

`mise bootstrap packages upgrade` re-resolves the configured formulae against the
formulae.brew.sh API and pours any whose current version differs from the
linked keg — the new keg replaces the old one and the links are repointed,
the same dance `brew upgrade` does. Since bottles only exist for a formula's
current version, "upgrade" and "install the current bottle" are the same
operation.

## Limitations

- **Cask artifact coverage is intentionally narrow.** `brew-cask` supports
  app bundles, binary artifacts, and simple pkg installers from dmg and common
  archive formats. Other artifact types, pkg installers without `pkgutil` IDs,
  and pkg installers with custom choices fail explicitly.
- **`brew services` is not implemented.**
- **Cask import/prune is not implemented.** `import` and `prune` are formulae-only
  until cask uninstall semantics can be made safe for app and pkg artifacts.
- **Source builds cover the common formula shapes.** mise's formula shim
  implements the widely-used subset of the DSL (see
  [Source formulae](#source-formulae)); formulae that reach beyond it fail
  with a clear error naming the unsupported feature.
- **Use canonical formula names.** `postgresql@17` is a formula name, not a
  mise version pin — the API's current stable version decides what gets
  installed. Aliases (`postgres`) install correctly but `mise bootstrap packages status`
  can't track them; mise warns and tells you the canonical name.
- `PATH` is up to you: `<prefix>/bin` must be on `PATH` to use linked
  binaries, just like with Homebrew itself.
