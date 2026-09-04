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

The artifact is chosen as the specification's consumer rules say: by OS, architecture, libc, and an archive format mise can unpack, preferring `tar.xz` over `tar.gz` over `zip` over a bare executable. Installer formats such as `deb`, `dmg`, and `msi` are never chosen. When two artifacts tie, mise refuses to guess; set `variant` to say which one you mean.

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

For a project on github.com, the versions are the repository's releases that carry a packslip, named by their tags with a leading `v` removed. At install time mise checks that the packslip inside the release agrees with the version the tag implied and refuses the release if it does not. For a project on its own domain, the versions are the entries of its signed release list, minus any the vendor has withdrawn; mise refuses a list that has expired.

Lockfiles record the artifact URL and its sha256 from the signed statement.
