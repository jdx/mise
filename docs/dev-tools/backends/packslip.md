# packslip Backend

The `packslip` backend installs tools from a vendor's signed release manifest.
The manifest names the release files, their digests, supported platforms, and
executable paths. mise verifies the signer and downloaded bytes, then installs
the artifact that fits your host.

::: warning
A project must publish packslips for its releases; this backend cannot install
arbitrary GitHub release assets. The [packslip format](https://packslip.dev) is
young, and its own version is what says what a manifest may contain.
:::

## Usage

With [mise activated](/getting-started.html), install packslip itself:

```sh
mise use -g packslip
packslip version
```

Omit `-g` to manage the tool in the current project's `mise.toml` instead.
For a project configuration:

```toml
[tools]
packslip = "latest"
```

The registry entry supplies Packslip's signer policy.

### Project names and discovery

A project name is a host followed by an optional path, without `https://`.
`packslip:owner/repo` is shorthand for `packslip:github.com/owner/repo`.

| Project form                         | Where mise looks                                                             |
| ------------------------------------ | ---------------------------------------------------------------------------- |
| `github.com/owner/repo`              | GitHub releases carrying `packslip.sigstore.json`.                           |
| `github.com/owner/repo/tools/mytool` | The same repository's releases, using `packslip.tools-mytool.sigstore.json`. |
| `tool.example.com`                   | `https://tool.example.com/.well-known/packslip.json`.                        |
| `example.com/tools/mytool`           | `https://example.com/.well-known/packslip/tools/mytool.json`.                |

A GitHub monorepo subpath identifies one tool, but the signing identity is
still pinned to the repository. The signed project and version must match
the requested tool and release, regardless of the bundle's filename.

For a domain project, obtain the vendor's public key through a trusted channel
and configure it explicitly. This example assumes the key file already exists:

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", pubkey = "/path/to/vendor.pub" }
```

The domain's signed list points to release bundles; their artifacts can live
on a different download host. A domain project without a signed list cannot
be installed. Recognizing a forge's signing issuer does not provide discovery:
GitHub has a release-API integration; other hosts need the signed-list location.

## Versions

List the project's available versions with:

```sh
mise ls-remote packslip
```

Packslip versions use semver, including compatible date versions such as
`2026.9.1`. mise uses that version to order releases and identify prereleases;
GitHub's release order and editable prerelease flag do not decide either.
Prereleases are excluded unless the `prerelease` tool option is enabled.

For GitHub discovery, mise reads versions from tags such as `v1.2.3`,
`mytool-v1.2.3`, or `v4.1` (normalized to `4.1.0`). Tags that do not map to a
version need an explicit mapping in a signed list. At installation, the
manifest's version must agree with the tag or list entry.

### Signed release lists

A GitHub repository can publish a supplementary signed list on its default
branch at `.well-known/packslip.json`, or `.well-known/packslip/<tool>.json`
for a monorepo tool. The list can withdraw a version, supply a bundle URL and
digest, or add a version that the release API does not expose.

Omitted versions can still come from GitHub releases: omission does not withdraw
them. For a domain project, the signed list supplies the entire release index.
A vendor withdrawal excludes a version even if a trusted stamper approved it.

### Release-list continuity and minimum age

mise rejects expired signed lists and sequences below the highest it has
accepted for the project. Once it has accepted a supplementary GitHub list,
that list disappearing is an error. A missing list cannot silently undo a
withdrawal. This continuity state is stored alongside the [signer pin](#pinned-signers).

If a [minimum release age](/configuration/settings.html#minimum_release_age)
is configured, discovery timestamps help filter candidates. Before downloading
an artifact, mise checks the verified transparency-log timestamp against the
effective cutoff. Only an explicitly allowed unlogged bundle uses the signed
publication timestamp instead.

## Latest

An unconstrained `latest` request considers recommendations in this order:

1. The vendor's `latest` pointer in an accepted signed release list.
2. GitHub's latest release, if there is no signed pointer.
3. The highest eligible semver, if there is no eligible recommendation.

The vendor can recommend an older supported release while a newer major
version exists. Prefix and channel requests keep their normal matching rules;
the recommendation does not reorder them.

A recommendation must still pass signature, identity, digest, release-age,
stamping, and host checks. A policy exclusion warns and tries another candidate.
An ineligible signed recommendation falls directly back to semver selection,
without consulting GitHub's pointer. Signature or digest failures and invalid,
expired, rolled-back, or unexpectedly missing lists stop resolution.

Resolution reaches for the vendor exactly as installation does: a stamped
candidate is fetched from the stamp's URL, and the vendor is asked only for a
withdrawal and a digest, so `latest` accepts every version `install` would.

Version listing and `latest` resolution read policy afresh rather than trusting
mise's remote-version cache, so withdrawals and trust changes take effect; what
they read is written back to that cache. Offline there is no policy to consult
and no network to consult it over, so both serve that cache like every other
backend, or nothing when it is empty; installing still rechecks.

## What is verified

Before unpacking a release, mise checks:

1. The bundle's signature and applicable certificate and transparency-log
   evidence against the expected repository identity or configured key.
2. The statement's structure, requested project and version, and any bundle
   digest recorded by a vendor list or trusted stamper.
3. Signer continuity, lockfile commitments, and applicable release-age policy.
4. The selected artifact's digest and size, plus any existing lockfile checksum.

The verified statement is retained as `.mise-packslip.json` in the install
directory. It supplies executable paths and metadata for resources.

Verification authenticates the signer's statement and the downloaded bytes.
It does not establish that the software is safe. A provenance link in the
manifest is separate evidence: this backend records its presence for continuity
checks but does not fetch and verify the linked build provenance.

### Artifact selection

mise uses signed metadata to select one artifact:

1. Match OS, architecture, and libc. An absent field leaves only that dimension
   unrestricted: a universal macOS build still requires macOS.
2. With a `variant`, consider only that variant. Without one, consider only
   artifacts that have no variant.
3. Keep formats mise can install, preferring the most specific platform match,
   then its archive/compression format preference over a bare executable.
4. Refuse an unresolved tie rather than guessing between builds.

Installer formats such as `deb`, `dmg`, and `msi` are not selected. A glibc host
with no matching GNU artifact may use a musl artifact; mise reports that
fallback in the debug log. The vendor must distinguish alternative builds with
variants; a client-side option cannot repair two identically described artifacts.

## Host requirements

After selecting an artifact, mise checks its declared requirements before
downloading it. Requirements do not break a selection tie or select another build.

| Requirement result                                                        | mise behavior                                      |
| ------------------------------------------------------------------------- | -------------------------------------------------- |
| Confirmed missing library, insufficient glibc, or insufficient OS version | Refuse installation.                               |
| Missing or outdated required command                                      | Warn and continue.                                 |
| A check cannot be completed                                               | Warn instead of assuming the host is incompatible. |

Command checks prefer active mise tools over ambient PATH, and either way mise
picks a path the OS can start, so a Windows `git.exe` or `node.cmd` counts and a
shebang-only script does not. Library detection is
platform-dependent; for example, an absent macOS library file may still exist
in the dyld shared cache, so mise reports that absence as unknown. On Linux the
OS version is the kernel release from `uname -r`, read up to the distribution's
suffix: `6.8.0-31-generic` is compared as `6.8.0`.

`ignore_requirements = true` allows a tool to install despite confirmed failures.
It does not supply the missing libraries or make an incompatible executable run.

## Pinned signers

mise preserves trust in two places:

| State                                                                                | What it protects                                                                                                                                |
| ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `packslip/pins.toml` under the [state directory](/directories.html#local-state-mise) | Previously accepted signers, signing scheme, vendor versus repackager status, provenance-link presence, and release-list continuity.            |
| `mise.lock`                                                                          | The project's signer and attestor commitment alongside each platform's artifact URL and checksum, including on another machine's first install. |

For a keyless signer, continuity compares the workflow path without its tag or
branch ref. A new release tag of the same workflow is the same signer. A new
workflow path or key requires an explicit trust decision. A signing-scheme change,
a vendor-to-repackager change, or lost provenance links can also be refused.

Inspect the local pins before investigating a trust failure:

```sh
mise packslip pins
mise packslip pins --json
```

After confirming a vendor's announced signer change, reset its local pin:

```sh
mise packslip forget github.com/owner/repo
```

This also resets remembered vendor-list continuity. It does not change an
explicit `pubkey` or identity option, erase stamper-list state, or remove the
signer commitment in `mise.lock`. Review those separately when accepting a
rotation. A conflicting lockfile entry must be removed and regenerated after
the new policy is configured; commit the reviewed change for other machines.

## Stamps

A stamper is a registry, mirror, or review service that publishes a signed list
of releases it admits. Configure the hosts you trust and the key or identity
allowed to sign each host's lists:

```toml
[settings.packslip]
stampers = [
  "stamps.example.com=/path/to/stamper.pub",
  "reviews.example.com=https://github.com/example/reviews/",
]
```

Each entry is `host=PIN`. The pin may be a minisign-format public key line,
a public-key file path, or a GitHub identity prefix. The example hosts and key
path are placeholders; replace them with a service and pin you trust.

A host publishes one list per project at
`https://<host>/.well-known/packslip/<project>.json`. With stampers configured:

- A version needs a non-yanked approval from at least one trusted host to be
  listed or installed. One host's withdrawal does not veto another's approval.
- A vendor withdrawal still excludes the release, regardless of stamps.
- mise checks the stamped bundle digest and the vendor-list digest, when present,
  then verifies the vendor signature. A stamp never replaces that signature.
- A stamp that records no digest is refused. The digest is what ties the host's
  review to a file; without one the entry admits a URL, not a bundle.
- Expired, rolled-back, invalid, or previously accepted but now missing stamper
  lists cause errors. Silently ignoring one would weaken the configured policy.

A stamper can mirror the exact vendor-signed bundle. Because the stamp already
names the bundle, mise asks the vendor only for what the vendor decides — a
withdrawal, and the digest they pinned — so a release asset deleted from GitHub
does not veto a mirror of a release that was never withdrawn. Re-signed
repackager bundles requiring a separate identity policy are not supported here.

No stamps are required when the setting is unset. To exempt one tool from
configured stampers, set `trust = "vendor"` as described below.

## Tool Options

These [tool options](/dev-tools/#tool-options) go in `[tools]` in `mise.toml`.
They apply to one tool; `packslip.exec`, `packslip.stampers`, and `skills.*`
are settings and belong under `[settings]`.

### `variant`

Choose a vendor-declared alternative build, such as `fips` or `baseline`.
Without this option, only unvarianted builds are considered.

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", variant = "fips" }
```

### `pubkey`

Pin a key-signed release using the minisign-format public-key line or a path to
its `.pub` file. The vendor list and release bundles must verify against it.

### `identity`, `identity_prefix`, `issuer`

Pin a keyless signer using an exact certificate identity or an identity prefix,
plus its OIDC issuer. These options can replace the policy derived from a forge
name. Keep the trailing slash when pinning a repository prefix.

### `list_identity_prefix`

When a different workflow signs the vendor's release list, pin its certificate
identity prefix separately. It replaces `identity` and `identity_prefix` only
for the vendor's list; release bundles still require their original signer.
The OIDC `issuer` is shared (including an issuer derived from a forge project).
Without this option, the list uses the same policy as release bundles.

The value must be a non-empty string and requires an issuer. It cannot be
combined with `pubkey`. It does not affect configured stampers, whose lists
use their own pins.

### `allow_unlogged`

Defaults to `false`. Set to `true` only when your policy accepts a vendor's
key-signed bundles without transparency-log evidence. This does not bypass
signature or artifact verification.

### `trust`

`trust = "vendor"` exempts the tool from configured stampers while retaining
vendor signature verification. The choice is recorded in lockfile options.

```toml
[tools]
"packslip:github.com/jdx/packslip" = { version = "latest", trust = "vendor" }
```

### `prerelease`

Set to `true` to include prerelease versions in selection.

### `ignore_requirements`

Set to `true` to override confirmed [host requirement](#host-requirements)
failures for this tool. Other verification checks still apply.

## Completions

For a command installed through this backend, `mise completion zsh --tool mytool
--install` installs a shell stub that follows the active version. See
[tool completions](/dev-tools/packslip-resources.html#completions) for shell
support, setup, and generated-script behavior.

## Skills

`mise skills ls` lists skills provided by active installed tools; `mise skills
sync` links them into an agent's skill directory. See
[agent skills](/dev-tools/packslip-resources.html#skills) for project setup,
automatic synchronization, and pruning.

### Resource selection and command execution

See [resource selection](/dev-tools/packslip-resources.html#resource-selection-and-command-execution)
for source priority, caching, and when mise runs a vendor executable.

## Troubleshooting

| Symptom                                      | What to check                                                                                                                    |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| No release or bundle found                   | Confirm that the project publishes packslips, the subpath is correct, and the tag maps to a version or appears in a signed list. |
| Nothing pins the signer                      | Supply `pubkey`, or a certificate identity/prefix and issuer, for a domain project.                                              |
| Signer or trust downgrade refused            | Inspect `mise packslip pins`, explicit tool options, and `mise.lock`; confirm the publisher's change before resetting trust.     |
| No eligible artifact or ambiguous artifacts  | Check the host and requested variant against the signed manifest. The publisher must fix an unresolved metadata tie.             |
| Signed list expired, rolled back, or missing | Obtain a current valid list from its publisher; removing an accepted list does not remove its policy.                            |
| Version excluded by stamp policy             | Check trusted hosts' non-yanked approvals and any vendor withdrawal.                                                             |
| Digest or size mismatch                      | Confirm the release and downloaded file; the bytes must match the signed statement.                                              |

Use `MISE_DEBUG=1` for selection and verification details. For resource-specific
failures, see [completions and skills troubleshooting](/dev-tools/packslip-resources.html#troubleshooting).

## Why publish one

A packslip lets a vendor describe each release's platforms and executable paths
with the release itself. Users can install through `packslip:` without a new
registry shorthand or a separate filename-matching recipe. It can also supply
versioned completions, CLI specifications, and agent skills.

The `github:` backend remains useful for releases without packslips, while
`aqua:` uses curated registry metadata. Publishing a packslip adds a signed
vendor description; it does not require replacing an existing release layout.

For GitHub releases, the `jdx/packslip@v0` action can sign and upload a bundle
after your workflow has built and uploaded the final artifacts. Follow the
[Packslip publishing guide](https://packslip.dev/docs/publishing/) for permissions,
inputs, version pinning, and monorepo setup. For domain hosting, also publish a
[signed release list](https://packslip.dev/docs/release-lists/).

The [Packslip specification](https://packslip.dev/release/v1/) defines the format.
The mise implementation is in
[`src/backend/packslip.rs`](https://github.com/jdx/mise/blob/main/src/backend/packslip.rs).
