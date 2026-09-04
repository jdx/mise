# packslip Backend

The `packslip` backend installs a release from the vendor's own signed release manifest, a [packslip](https://packslip.dev). One file per release, `packslip.sigstore.json`, says what shipped and how to verify it: checksums, platforms, executables, and provenance links, in a standard sigstore bundle. mise verifies it against the identity the project name implies and installs exactly what it lists, so nothing is guessed from file names and no registry entry is needed.

The code for this is inside of the mise repository at [`./src/backend/packslip.rs`](https://github.com/jdx/mise/blob/main/src/backend/packslip.rs).

::: warning
packslip is a work-in-progress proposal. The format still changes between releases, and few projects publish one yet.
:::

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

## Versions

A packslip version is semver, so mise ranks a project's versions by semver precedence whatever order GitHub lists its releases in, and treats a version with a prerelease part (`1.3.0-rc.1`, `1.4.0-nightly.20260904`) as a prerelease whatever the GitHub release's own flag says. Prereleases are skipped unless the `prerelease` tool option is set.

For a project on github.com, the versions are the repository's releases that carry a packslip, named by their tags as the specification reads them: the version itself, optionally after a `v`, and optionally after the tool's subpath, its last segment, or the repository name plus a separator (`v1.2.3`, `jq-1.7.1`, `oxlint_v1.0.0`), with loose spellings such as `v4.1` normalized to `4.1.0`. A tag that names no version is skipped. At install time mise checks that the packslip inside the release agrees with the version the tag named and refuses the release if it does not.

The repository may also keep a signed list at `.well-known/packslip.json` (or `.well-known/packslip/<tool>.json` for a monorepo tool) on its default branch, signed by the same identity as its packslips. When it does, a version the list marks withdrawn is dropped, a version it names has its packslip fetched from the URL the list gives and checked against the digest it signed, and a version it lists that the releases endpoint lacks is added.

For a project on its own domain, the versions are the entries of its signed release list, minus any the vendor has withdrawn. For either kind of list mise refuses one that has expired, and refuses one whose sequence is below the highest it has accepted for the project, which it remembers under its state directory.

Lockfiles record the artifact URL and its sha256 from the signed statement.
