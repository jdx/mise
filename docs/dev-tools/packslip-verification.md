# Packslip Verification and Policy

mise uses Packslip's signed metadata to discover releases, verify publishers,
and select compatible builds. This guide describes the discovery and policy
rules behind the [Packslip backend](/dev-tools/backends/packslip.html).
For installation examples and tool options, start with that page.

A **manifest** describes a release, a **bundle** contains that manifest and its
signature evidence, and an **artifact** is a downloadable build named by the
manifest. A **signed release list** indexes versions and can recommend or
withdraw them.

## Project discovery

| Project form                         | Where mise looks                                                        |
| ------------------------------------ | ----------------------------------------------------------------------- |
| `github.com/owner/repo`              | GitHub releases carrying `packslip.sigstore.json`.                      |
| `github.com/owner/repo/tools/mytool` | The repository's releases, using `packslip.tools-mytool.sigstore.json`. |
| `tool.example.com`                   | `https://tool.example.com/.well-known/packslip.json`.                   |
| `example.com/tools/mytool`           | `https://example.com/.well-known/packslip/tools/mytool.json`.           |

A GitHub monorepo subpath identifies one tool, but the signing identity is still
pinned to the repository. The signed project and version must match the requested
tool and release, regardless of the bundle's filename.

For domain projects, the signed list supplies bundle URLs; artifacts may live
on another download host. A domain without a signed list cannot be installed.
Recognizing a forge's signing issuer does not provide release discovery:
GitHub has a release-API integration; other hosts need the signed-list location.

## Version resolution

Packslip versions use semantic versioning, including compatible date versions
such as `2026.9.1`. The version determines ordering and prerelease status;
GitHub's release order and editable prerelease flag do not determine either.

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
that list disappearing is an error. This prevents a missing list from silently
undoing a withdrawal. The remembered list state is stored alongside the
[signer pin](#signer-continuity).

With [minimum release age](/configuration/settings.html#minimum_release_age)
enabled, discovery timestamps help filter candidates. Before downloading an
artifact, mise checks the verified transparency-log timestamp against the
effective cutoff. Only an explicitly allowed unlogged bundle uses the signed
publication timestamp instead.

### Recommendations and fallback

For an unconstrained `latest` request, mise considers the vendor's signed
recommendation first, then GitHub's latest release if there is no signed pointer.
Without an eligible recommendation, mise selects the highest eligible semantic
version. Prefix and channel requests keep their normal matching rules; the
recommendation does not reorder them.

A recommendation must pass signature, identity, digest, release-age, stamping,
and host checks. A policy exclusion warns and tries another candidate. An
ineligible signed recommendation falls directly back to semantic-version
selection, without consulting GitHub's pointer. Signature or digest failures
and invalid, expired, rolled-back, or unexpectedly missing lists stop resolution.

For a stamped release, mise fetches the bundle from the stamp's URL and checks
the vendor's list for withdrawals and any recorded digest. Resolution and
installation use the same source and verification policy.

### Caching and offline use

Online version listing and `latest` resolution read policy afresh so withdrawals
and trust changes take effect. They write the results to mise's remote-version
cache. Offline, both use that cache, or return no versions if it is empty.
Installation still rechecks verification policy.

## Verification checks

Before unpacking a release, mise checks:

1. The bundle's signature and applicable certificate and transparency-log
   evidence against the expected repository identity or configured key.
2. The manifest's structure, requested project and version, and any bundle
   digest recorded by a vendor list or trusted stamper.
3. Signer continuity, lockfile commitments, and applicable release-age policy.
4. The selected artifact's digest and size, plus any existing lockfile checksum.

The verified manifest is retained as `.mise-packslip.json` in the install
directory. It supplies executable paths and metadata for resources.

Verification authenticates the signer and downloaded bytes. A provenance link
in the manifest is separate evidence: mise records its presence for continuity
checks but does not fetch and verify the linked build provenance.

## Signer continuity

mise preserves trust in two places:

| State                                                                                | What it records                                                                                                                                 |
| ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `packslip/pins.toml` under the [state directory](/directories.html#local-state-mise) | Previously accepted signers, signing scheme, vendor versus repackager status, provenance-link presence, and release-list continuity.            |
| `mise.lock`                                                                          | The project's signer and attestor commitment alongside each platform's artifact URL and checksum, including on another machine's first install. |

For a keyless signer, continuity compares the workflow path without its tag or
branch ref. A new release tag of the same workflow is the same signer. A new
workflow path or key requires an explicit trust decision. A signing-scheme change,
a vendor-to-repackager change, or lost provenance links can also be refused.

See [signer changes](/dev-tools/backends/packslip.html#pinned-signers) for
inspection and reset commands, including how explicit options and lockfile
commitments affect a rotation.

## Stamps

A stamper is a registry, mirror, or review service that publishes a signed list
of releases it approves. No stamps are required by default. To require them,
configure the hosts you trust and the key or identity allowed to sign each
host's lists:

```toml
[settings.packslip]
stampers = [
  "stamps.example.com=/path/to/stamper.pub",
  "reviews.example.com=https://github.com/example/reviews/",
]
```

Each entry is `host=PIN`. The pin may be a minisign-format public-key line,
a public-key file path, or a GitHub identity prefix. Replace the example hosts
and key path with a service and pin you trust.

A host publishes one list per project at
`https://<host>/.well-known/packslip/<project>.json`. With stampers configured:

- A version needs a non-yanked approval from at least one trusted host to be
  listed or installed. One host's withdrawal does not veto another's approval.
- A vendor withdrawal still excludes the release, regardless of stamps.
- mise checks the stamped bundle digest and the vendor-list digest, when present,
  then verifies the vendor signature. A stamp never replaces that signature.
- A stamp without a digest is refused: the approval must identify the bundle's
  contents, not just its URL.
- Expired, rolled-back, invalid, or previously accepted but now missing stamper
  lists cause errors.

To exempt one tool while retaining vendor verification, set its
[`trust = "vendor"`](/dev-tools/backends/packslip.html#trust) tool option.

### Mirrors

A stamper can mirror the exact vendor-signed bundle. mise fetches it from the
stamp's URL and checks the vendor's list for withdrawals and any recorded digest.
Deleting a GitHub release asset does not block an approved mirror of a release
that the vendor has not withdrawn. Re-signed repackager bundles requiring a
separate identity policy are not supported here.

## Artifact selection

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
fallback in the debug log. The publisher must distinguish alternative builds
with variants. A client option cannot resolve two identically described artifacts.

## Host requirements

After selecting an artifact, mise checks its declared requirements before
downloading it. Requirements do not break a selection tie or select another build.

| Requirement result                                                        | mise behavior                                      |
| ------------------------------------------------------------------------- | -------------------------------------------------- |
| Confirmed missing library, insufficient glibc, or insufficient OS version | Refuse installation.                               |
| Missing or outdated required command                                      | Warn and continue.                                 |
| A check cannot be completed                                               | Warn instead of assuming the host is incompatible. |

Command checks prefer active mise tools over ambient PATH. mise checks for a
path the OS can execute: on Windows, `git.exe` or `node.cmd` counts, but a
shebang-only script does not.

Library detection is platform-dependent. For example, an absent macOS library
file may still exist in the dyld shared cache, so mise reports that absence as
unknown. On Linux, the OS version is the kernel release from `uname -r`, read up
to the distribution's suffix: `6.8.0-31-generic` is compared as `6.8.0`.

The [`ignore_requirements`](/dev-tools/backends/packslip.html#ignore-requirements)
tool option permits installation despite confirmed failures. It does not supply
missing libraries or make an incompatible executable run.
