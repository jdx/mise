---
description: Understand how mise finds packslip releases, verifies their publishers, enforces trust policies, and picks a build for your platform.
socialDescription: How mise finds, verifies, and selects packslip releases.
---

# Verification and policy

The [packslip backend](/dev-tools/backends/packslip.html) uses signed metadata
to find releases, verify the publisher, and pick a build for your platform, and
[`mise self-update`](#self-update) uses the same checks for mise itself. For
tool options and term definitions, see the backend page; for fixes to specific
errors, see [troubleshooting](/dev-tools/backends/packslip.html#troubleshooting).

A **signed release list** is a publisher-signed index of a project's versions.
It can recommend or withdraw versions and point at their bundles.

## Project discovery

| Project form                         | Where mise looks                                                        |
| ------------------------------------ | ----------------------------------------------------------------------- |
| `github.com/owner/repo`              | GitHub releases carrying `packslip.sigstore.json`.                      |
| `github.com/owner/repo/tools/mytool` | The repository's releases, using `packslip.tools-mytool.sigstore.json`. |
| `tool.example.com`                   | `https://tool.example.com/.well-known/packslip.json`.                   |
| `example.com/tools/mytool`           | `https://example.com/.well-known/packslip/tools/mytool.json`.           |

A GitHub monorepo subpath identifies one tool, but the signing identity is still
pinned to the repository. The signed project and version must match the tool
and release you requested, whatever the bundle's filename.

For a domain project, the signed list supplies the bundle URLs, and artifacts
can live on another download host. A domain without a signed list cannot be
installed. Recognizing a forge's signing issuer does not provide release
discovery: GitHub has a release-API integration, and other hosts need a signed
list.

## Version resolution

The packslip format requires semantic versions, which include date versions
such as `2026.9.1`. The version decides ordering and prerelease status; GitHub's
release order and its editable prerelease flag decide neither.

For GitHub discovery, mise reads versions from tags such as `v1.2.3`,
`mytool-v1.2.3`, or `v4.1` (read as `4.1.0`). A tag that does not map to a
version needs an explicit mapping in a signed list. At installation, the
manifest's version must agree with the tag or list entry.

### Signed release lists

A GitHub repository can publish a supplementary signed list on its default
branch at `.well-known/packslip.json`, or `.well-known/packslip/<tool>.json` for
a monorepo tool. The list can withdraw a version, supply a bundle URL and
digest, or add a version that the release API does not show.

Versions the list omits can still come from GitHub releases; omission does not
withdraw them. For a domain project, the signed list is the entire release
index. A vendor withdrawal excludes a version even if a trusted stamper approved
it.

### Recommendations and fallback

For an unconstrained `latest` request, mise tries the vendor's signed
recommendation first, then GitHub's latest release if there is no signed
pointer. Without an eligible recommendation, mise selects the highest eligible
semantic version. Prefix and channel requests keep their normal matching rules;
the recommendation does not reorder them.

A publisher can recommend an older supported release even when a newer major
version exists, so `latest` is not always the highest version listed.

A recommendation must pass the signature, identity, digest, release-age, stamp,
and host checks. A policy exclusion warns and tries the next candidate. An
ineligible signed recommendation falls straight back to semantic-version
selection, without consulting GitHub's pointer. Signature or digest failures,
and lists that are invalid, expired, rolled back, or unexpectedly missing, stop
resolution.

### Caching and offline use

Online version listing and `latest` resolution read policy afresh, so
withdrawals and trust changes take effect, and they write the results to mise's
remote-version cache. Offline, both use that cache, or return no versions if it
is empty. Installation still rechecks verification policy, so a cached version
list is not enough for an offline install: the bundles, artifacts, and trust
evidence it needs must also be available.

## Verification checks

Before unpacking a release, mise checks:

1. The bundle's signature, and its certificate and transparency-log evidence
   where they apply, against the expected repository identity or configured key.
2. The manifest's structure, the requested project and version, and any bundle
   digest recorded by a vendor list or trusted stamper.
3. Signer continuity, lockfile commitments, and the release-age policy that
   applies.
4. The selected artifact's digest and size, plus any checksum in the lockfile.

The verified manifest is kept as `.mise-packslip.json` in the install directory.
It supplies executable paths and the metadata for
[man pages, completions, and skills](/dev-tools/packslip-resources.html).

Verification authenticates the signer and the downloaded bytes. A provenance
link in the manifest is separate evidence: mise records whether one is present
for continuity checks, but it does not fetch or verify the linked build
provenance.

## Signer continuity

mise keeps trust decisions in two places:

| State                                                                             | What it records                                                                                                                                                  |
| --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packslip/pins.toml` in the [state directory](/directories.html#local-state-mise) | Signers accepted before, signing scheme, vendor or repackager status, whether provenance links were present, forge repository IDs, and release-list state.       |
| `mise.lock`                                                                       | The project's signer, attestor, and forge-ID commitment beside each platform's artifact URL and checksum, which also applies on another machine's first install. |

For a keyless signer, continuity compares the workflow path without its tag or
branch ref, so a new release tag of the same workflow is the same signer. A new
workflow path or key needs an explicit trust decision. A change of signing
scheme, a change from vendor to repackager, or provenance links that disappear
can also be refused.

Deleting `pins.toml` resets local continuity for every recorded project. It is
not a routine fix for an installation failure, and it does not remove signer
commitments from a project's lockfile. To accept a rotation for one project,
follow [signer changes](/dev-tools/backends/packslip.html#pinned-signers).

### Renamed, transferred, and re-created repositories

A GitHub or GitLab project's name locates it, but the forge's repository ID
identifies it. GitHub Actions and GitLab CI signing certificates record that ID,
and it does not change when a repository is renamed or transferred to another
owner. mise pins the ID with the signer and compares each release's ID with the
pin:

| What happened to the name                          | Result                                                                                                                    |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Renamed, or transferred to another owner           | Installs. mise warns once that the project has a new name, and the pin follows it to that name.                           |
| Deleted and re-created, by anyone, under that name | Refused once the original is pinned, even though the name and workflow path match: the new repository has a different ID. |

The owner is not part of the identity: only a repository's current owner can
transfer it, and that owner already signs its releases.

Signer continuity then compares the workflow's path inside the repository, so
`github.com/old/tool/.github/workflows/release.yml` continues as
`github.com/new/tool/.github/workflows/release.yml`. Releases published before a
rename are signed under the old name and still install when the configuration
names the new one.

mise finds the pin by the repository ID in the release's certificate, not by
name. So if your configuration already uses the new name, or this machine never
saw the rename, the old name's pin still applies: the release must match its
signer, provenance, and attestor. After mise accepts the release, it moves the
pin and its release-list state to the new name, so each repository keeps one
pin. Errors name the pin as it is recorded; pass that name to
`mise packslip forget`.

With no pin yet and a release signed under another name, mise asks the forge
which repository the requested name now belongs to. If it cannot ask, for
example offline or when rate-limited, it compares names only and refuses the
release.

This is still trust on first use. The first install on a machine with no pin and
no lockfile entry accepts whichever repository owns the name at that moment, so
a name taken over before that install is not detected. Commit `mise.lock` so
every machine starts from the IDs the project accepted.

## Release-list continuity {#release-list-continuity-and-minimum-age}

mise rejects an expired signed list, and a list whose sequence is below the
highest it has accepted for the project. Once it has accepted a supplementary
GitHub list, that list disappearing is an error, so a missing list cannot
silently undo a withdrawal. The list state is stored with the
[signer pin](#signer-continuity), and for a GitHub or GitLab project it follows
the repository ID through a
[rename](#renamed-transferred-and-re-created-repositories) the way the pin does:
a list accepted under the old name still sets the lowest sequence, and still may
not disappear, under the new one.

## Minimum release age

[`minimum_release_age`](/configuration/settings.html#minimum_release_age)
applies to packslip tools as to other backends; see
[Minimum release age](/security.html#minimum-release-age) for the setting
itself. Discovery timestamps filter candidates first. Before downloading an
artifact, mise checks the verified transparency-log timestamp against the
cutoff; only an explicitly allowed unlogged bundle uses the signed publication
timestamp instead.

The cutoff decides which release a version request may pick, so it does not
apply to a release that is already chosen. An exact pin such as
`"packslip:github.com/jdx/hk" = "2.0.1"` and a version recorded in `mise.lock`
both install and lock while the release is still within the cutoff. Requests
such as `latest` or `2` wait until a release is old enough.

## Stamps

A stamper is a registry, mirror, or review service that publishes a signed list
of the releases it approves. No stamps are required by default. To require them,
set [`packslip.stampers`](/configuration/settings.html#packslip.stampers) to the
hosts you trust and the key or identity allowed to sign each host's lists:

```toml
[settings.packslip]
stampers = [
  "stamps.example.com=/path/to/stamper.pub",
  "reviews.example.com=https://github.com/example/reviews/",
]
```

Each entry is `host=PIN`. The pin can be a minisign-format public-key line, the
path of a public-key file, or a GitHub identity prefix. Replace the example
hosts and key path with a service and pin you trust.

A host publishes one list per project at
`https://<host>/.well-known/packslip/<project>.json`. With stampers configured:

- A version needs a non-yanked approval from at least one trusted host to be
  listed or installed. One host's withdrawal does not veto another's approval.
- A vendor withdrawal still excludes the release, regardless of stamps.
- mise checks the stamped bundle digest and the vendor-list digest, when
  present, then verifies the vendor signature. A stamp never replaces that
  signature.
- A stamp without a digest is refused: the approval must identify the bundle's
  contents, not just its URL.
- Expired, rolled-back, or invalid stamper lists, and lists accepted before that
  are now missing, cause errors.

To exempt one tool while keeping vendor verification, set its
[`trust = "vendor"`](/dev-tools/backends/packslip.html#trust) tool option.

### Mirrors

A stamper can mirror the exact vendor-signed bundle. mise fetches a stamped
release from the stamp's URL and checks the vendor's list for withdrawals and
any recorded digest; resolution and installation use the same source and
policy. Deleting a GitHub release asset does not block an approved mirror of a
release the vendor has not withdrawn. Re-signed repackager bundles, which would
need a separate identity policy, are not supported.

## Artifact selection

mise uses the signed metadata to select one artifact:

1. Match OS, architecture, and libc. An absent field leaves only that dimension
   unrestricted: a universal macOS build still requires macOS.
2. With a `variant`, consider only that variant. Without one, consider only
   artifacts that have no variant.
3. Drop formats mise cannot install. Prefer the most specific platform match;
   between equally specific builds, prefer an archive over a bare executable.
4. Refuse an unresolved tie instead of guessing between builds.

Installer formats such as `deb`, `dmg`, and `msi` are never selected. A glibc
host can use a matching static musl artifact when there is no GNU artifact, or
when the selected GNU artifact declares a `glibc_min` above the host's detected
version; mise reports that fallback in the debug log. Publishers distinguish
alternative builds with variants, and no client option can choose between two
identically described artifacts.

## Host requirements

After selecting an artifact, mise checks its declared requirements before
downloading it. Requirements do not break a selection tie, and apart from the
musl fallback above they never select another build.

| Requirement result                                                                           | mise behavior                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Insufficient glibc with a matching static musl artifact                                      | Select the musl artifact.                          |
| Confirmed missing library, insufficient glibc without a fallback, or insufficient OS version | Refuse installation.                               |
| Missing or outdated required command                                                         | Warn and continue.                                 |
| A check cannot be completed                                                                  | Warn instead of assuming the host is incompatible. |

Command checks prefer active mise tools over the ambient `PATH`. mise looks for
a path the OS can execute: on Windows, `git.exe` or `node.cmd` counts, but a
shebang-only script does not.

Library detection depends on the platform. For example, a macOS library that is
absent from disk can still exist in the dyld shared cache, so mise reports that
absence as unknown. On Linux, the OS version is the kernel release from
`uname -r`, read up to the distribution's suffix: `6.8.0-31-generic` is compared
as `6.8.0`.

The [`ignore_requirements`](/dev-tools/backends/packslip.html#ignore-requirements)
tool option installs despite confirmed failures and keeps the GNU build instead
of falling back to musl.

## How mise verifies its own updates {#self-update}

[`mise self-update`](/cli/self-update.html) and automatic updates check mise's
own releases with packslip before replacing the running binary. Every release
from 2026.9.3 on must carry a `packslip.sigstore.json` bundle. If it is missing
or fails any check below, the update stops and the installed mise stays as it
is. mise checks that:

- The bundle's Sigstore signature and transparency-log entry are valid.
- The certificate names mise's GitHub repository by its built-in repository ID,
  so renaming the repository or moving it to another organization does not change
  what is trusted.
- The bundle was signed by the repository's `.github/workflows/release.yml`
  workflow, is the vendor's own, and is for the version being installed.
- The downloaded archive matches the signed digest.
- The transparency-log timestamp is older than
  [`self_update.minimum_release_age`](/configuration/settings.html#self_update.minimum_release_age).
  Naming a version, as in `mise self-update 2026.10.4`, skips this check.

The archive's embedded signature is checked too, for every release; it is the
only check for releases before 2026.9.3. A mirror configured with
[`self_update.repository`](/configuration/settings.html#self_update.repository)
must serve the original manifest and archive bytes, so it supplies files but
cannot change what is trusted. To install mise itself with the `packslip` CLI,
see [Installing mise](/installing-mise.html#packslip).
