# packslip Backend <Badge type="warning" text="experimental" />

The `packslip` backend installs a release from the vendor's own signed release manifest, a [packslip](https://packslip.dev). One file per release, `packslip.sigstore.json`, says what shipped and how to verify it: checksums, platforms, executables, and provenance links, in a standard sigstore bundle. mise verifies it against the identity the project name implies and installs exactly what it lists, so nothing is guessed from file names and no registry entry is needed.

The code for this is inside of the mise repository at [`./src/backend/packslip.rs`](https://github.com/jdx/mise/blob/main/src/backend/packslip.rs).

::: warning
packslip is a work-in-progress proposal and this backend is experimental: enable it with `mise settings experimental=true`. The format still changes between releases, and few projects publish one yet.
:::

## Why publish one

The backends most people use today work from the outside of a release:

- `github:owner/repo` picks an asset by scoring file names against the host (OS, arch, libc, format). That holds until a project renames its assets, ships two builds for one platform, or puts the executable somewhere unexpected; then every user needs `asset_pattern` or `bin` in their config. Nothing ties the file to the project unless the project also publishes checksums or provenance in a form mise recognises.
- `aqua:owner/repo` reads a registry entry that volunteers maintain for the project, with checksums and, for some packages, cosign or SLSA checks. When a project changes its asset names the entry is wrong until someone sends a fix ([pnpm v11](https://github.com/aquaproj/aqua-registry/pull/52822), [doggo v1.2.0](https://github.com/aquaproj/aqua-registry/pull/55890), [go-jsonnet v0.22.0](https://github.com/aquaproj/aqua-registry/pull/50942), [wrkflw v0.8.0](https://github.com/aquaproj/aqua-registry/pull/59777)), and the snapshot built into mise picks it up a release later.
- `packslip:owner/repo` reads what the project itself published with the release, signed by the project. Nothing to guess, nobody to wait for.

For a vendor:

- **A direct line to your users.** You say which file is for which platform, which executables it holds and what they are called on PATH, and what the host needs. Your users get that with the release itself, not after someone updates a registry, and when you change a layout or a naming scheme the next packslip says so and no install breaks.
- **You ship more than binaries.** Shell completions, man pages, a [usage](https://usage.jdx.dev) spec of the CLI, and [agent skills](https://packslip.dev/release/v1/#resources), each declared once and installed in the version the user actually has.
- **On GitHub, one step in the release job.** `uses: jdx/packslip@v1` with the artifact glob signs the manifest keylessly with the workflow's own identity, attests build provenance, and uploads it. There is no key to create, store, or rotate. On another forge, or a plain web server, `packslip create` signs with a key you hold and you publish a signed release list at a well-known URL on your domain; users pin the key in their config.
- **Nothing between you and the user can swap the bytes.** A mirror, a proxy, or a compromised download host cannot pass off other bytes as your release, because the signer is your workflow or your key.

For a user asking a vendor for one:

- **The name says who may sign, the way `~/.ssh/known_hosts` says which key a host must present.** `packslip:github.com/owner/repo` accepts only a packslip signed by that repository's release workflow through GitHub's issuer; the signature, its transparency log entry, the statement, and the artifact's digest and size are all checked before anything is unpacked.
- **The right artifact, with no registry to update first.** musl and gnu, universal binaries, a `fips` or `baseline` variant, the executable's path inside the archive: all from the manifest the vendor wrote when it cut the release, so nobody plays catch-up after a rename.
- **Completions and skills that match the version in use**, from the vendor, with no per-tool setup in mise or in your shell config.
- **Upgrades are checked the way first installs are.** A checksum in `mise.lock` proves one download is the same file it was the first time. A packslip proves the next version came from the same signer as well, so an upgrade on a teammate's machine or in CI is checked against the project's identity rather than against a hash nobody could verify when it was first recorded.

The specification and the `packslip` CLI are at [packslip.dev](https://packslip.dev).

## Usage

A project is named the way Go names a module: a host, then a path. On github.com the name is also the pin: the packslip must be signed by a workflow of that repository, through GitHub's OIDC issuer.

```sh
$ mise use -g packslip:github.com/jdx/packslip
$ packslip version
packslip 0.2.0
```

`packslip:owner/repo` means the same as `packslip:github.com/owner/repo`. A tool in a monorepo adds its subpath, `packslip:github.com/oxc-project/oxc/oxlint`, and mise reads that tool's own `packslip.oxlint.sigstore.json` from the shared release.

A project on its own domain publishes a signed release list at `https://<host>/.well-known/packslip/<path>.json`. Nothing implies its signer, so pin it yourself:

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", pubkey = "RWQ..." }
```

## What is verified

1. The bundle's signature, certificate chain, and transparency log entry, as sigstore defines them, against the pinned identity or key.
2. The statement's structure, and that it is for the project you asked for at the version you asked for.
3. The digest and size of the one artifact selected for this host, from the signed statement, before it is unpacked.

Only after that does mise unpack the artifact and put the executables the packslip names on PATH. The verified statement is kept in the install directory as `.mise-packslip.json`.

The artifact is chosen as the specification's consumer rules say. An artifact fits when each of its `os`, `arch`, and `libc` is absent or equal to the host's, so a universal macOS binary, a jar, or a script fits everywhere; among those that fit, the one naming the most of those three wins, so a build for the host beats a portable one; then mise's format preference decides, archives first (`tar.xz`, `tar.zst`, `tar.gz`, `tar.bz2`, `tar`, `zip`, `7z`), then a single compressed executable (`xz`, `zst`, `gz`, `bz2`), then a bare one. Installer formats such as `deb`, `dmg`, and `msi` are never chosen. When two artifacts still tie, mise refuses to guess; set `variant` to say which one you mean. A glibc host with no gnu build takes a musl build, which is static, and says so in the debug log.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `packslip` backend—these go in `[tools]` in `mise.toml`.

### `variant`

Selects one of several builds the vendor publishes for one platform, such as `fips` or `baseline`. Without it, only artifacts with no variant are considered.

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", variant = "fips" }
```

### `pubkey`

For a project on its own domain: the vendor's public key, as the minisign-format line from its `.pub` file or the path of that file. The release list and every packslip must be signed with it.

### `identity`, `identity_prefix`, `issuer`

For a project on its own domain signed keylessly, or to override the identity a forge name implies: the exact certificate identity, or a prefix it must start with, and the OIDC issuer.

### `allow_unlogged`

Accept a key-signed bundle that carries no transparency log entry, which a vendor produces for an air-gapped release. Off by default; turn it on only for a vendor you have agreed to accept that from.

## Completions

A packslip may list what the release ships besides its executables: shell completions, a spec of the CLI, an agent skill. mise reads those `resources` for tools it installed this way.

```sh
mise completion zsh --tool rg           # print the script
mise completion zsh --tool rg --install # put a stub where zsh looks
```

The script comes from whichever version of the tool is active in the current directory. A vendor whose layouts differ by platform scopes an entry with `os`, `arch`, or `libc`; mise keeps the entries that apply to the artifact it installed and, of those, the most specific. It then takes the most verifiable source the vendor offered, in the order the specification gives: a file inside the artifact or a separate signed asset, a file from the source repository at the release's commit, a script derived from the tool's [usage](https://usage.jdx.dev) spec with the `usage` command, and only then a command of the tool's own. That last kind runs a freshly installed binary before you have run it yourself, so mise refuses it unless the [`packslip.exec`](/configuration/settings#packslip-exec) setting is on.

`--install` writes a stub, not the script. Completions are global shell state while the active version depends on the directory, so the stub asks mise for the script when the shell completes the tool. In zsh and bash the vendor's script takes over for one completion and the stub is put back afterwards, so a version switch in another directory is followed on the next tab; fish and PowerShell load the script once per shell session. The file goes where the shell loads completions by name, the same place `mise completion zsh --install` puts mise's own, and the command prints any one-time line your shell still needs.

Files the vendor keeps outside the artifact (a separate release asset, or a path in the repository) are fetched at install time. An asset must match the digest the packslip signed; a repository file is pinned by the release's commit. Completions derived from a usage spec call `usage complete-word` at shell runtime, so install `usage` alongside such tools (`mise use -g usage`).

## Versions

A packslip version is semver, so mise ranks a project's versions by semver precedence whatever order GitHub lists its releases in, and treats a version with a prerelease part (`1.3.0-rc.1`, `1.4.0-nightly.20260904`) as a prerelease whatever the GitHub release's own flag says. Prereleases are skipped unless the `prerelease` tool option is set.

For a project on github.com, the versions are the repository's releases that carry a packslip, named by their tags as the specification reads them: the version itself, optionally after a `v`, and optionally after the tool's subpath, its last segment, or the repository name plus a separator (`v1.2.3`, `jq-1.7.1`, `oxlint_v1.0.0`), with loose spellings such as `v4.1` normalized to `4.1.0`. A tag that names no version is skipped. At install time mise checks that the packslip inside the release agrees with the version the tag named and refuses the release if it does not.

The repository may also keep a signed list at `.well-known/packslip.json` (or `.well-known/packslip/<tool>.json` for a monorepo tool) on its default branch, signed by the same identity as its packslips. When it does, a version the list marks withdrawn is dropped, a version it names has its packslip fetched from the URL the list gives and checked against the digest it signed, and a version it lists that the releases endpoint lacks is added.

For a project on its own domain, the versions are the entries of its signed release list, minus any the vendor has withdrawn. For either kind of list mise refuses one that has expired, and refuses one whose sequence is below the highest it has accepted for the project, which it remembers under its state directory.

Lockfiles record the artifact URL and its sha256 from the signed statement.
