# brew <Badge type="warning" text="experimental" />

Homebrew formulae — **without requiring Homebrew to be installed**.

```toml
[system.packages]
brew = ["postgresql@17", "ffmpeg", "imagemagick"]
```

mise installs [homebrew/core](https://formulae.brew.sh) formulae directly
into the canonical Homebrew prefix — `/opt/homebrew` on arm64 macOS,
`/home/linuxbrew/.linuxbrew` on Linux. It fetches metadata from the
formulae.brew.sh API, resolves the runtime dependency closure, downloads
prebuilt bottles from ghcr.io (verifying sha256 checksums), and performs the
same relocation, code-signing, and linking work `brew` does when pouring a
bottle. mise never shells out to `brew`.

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
there. On Linux, formulae without a bottle for your architecture fail with a
clear error (arm64 Linux bottles exist for most but not all of homebrew/core).

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

## Limitations

- **Formulae only.** Casks (GUI apps) and `brew services` are not
  implemented.
- **No taps.** Third-party taps are Ruby code that requires Homebrew to
  evaluate; only homebrew/core is supported.
- **No source builds.** Formulae without a bottle for your platform fail
  with a clear error.
- **Use canonical formula names.** `postgresql@17` is a formula name, not a
  mise version pin — the API's current stable version decides what gets
  installed. Aliases (`postgres`) install correctly but `mise system status`
  can't track them; mise warns and tells you the canonical name.
- `PATH` is up to you: `<prefix>/bin` must be on `PATH` to use linked
  binaries, just like with Homebrew itself.
