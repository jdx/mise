---
description: Install tools from signed packslip releases, with publisher verification, checksum checks, and version-matched completions and skills.
socialDescription: Install tools from signed packslip releases with verified downloads.
---

# packslip backend

The `packslip:` backend installs tools from release manifests that their
publishers sign with [packslip](https://packslip.dev). mise verifies the
publisher and the download, picks the build for your platform, and installs its
executables. Support is built into mise, so you do not need the `packslip` CLI.

A publisher signs a **manifest** that lists each downloadable build, an
**artifact**, with its digest. The manifest and its signature evidence travel
together as a **bundle**. A release can also declare man pages, shell
completions, and agent skills for the tool.

Use this backend when a tool's publisher signs its releases with packslip; the
registry already prefers it for those tools. For other tools, use
[aqua](/dev-tools/backends/aqua.html), [GitHub](/dev-tools/backends/github.html),
or another [backend](/dev-tools/backends/). For background, read
[Introducing packslip](https://jdx.dev/posts/2026-09-05-introducing-packslip/).

## Quick start {#usage}

Install [hk](https://hk.jdx.dev), a git hook and lint manager, in your project:

```sh
mise use packslip:github.com/jdx/hk
mise exec -- hk --version
```

`mise use` installs hk and adds it to the project's `mise.toml`:

```toml
[tools]
"packslip:github.com/jdx/hk" = "latest"
```

If you edit `mise.toml` by hand instead, run `mise install`. To install hk for
every directory, use `mise use -g packslip:github.com/jdx/hk`. The registry
shorthand `mise use hk` selects this backend for hk 1.58.1 and later.

## Project identifiers {#project-names-and-discovery}

Write `packslip:` followed by the project's host and path, without `https://`.
For GitHub you can leave out the host: `packslip:jdx/hk` is the same project as
`packslip:github.com/jdx/hk`.

| Identifier                                    | Source                                         |
| --------------------------------------------- | ---------------------------------------------- |
| `packslip:github.com/owner/repo`              | A GitHub repository's releases.                |
| `packslip:github.com/owner/repo/tools/mytool` | One tool in a GitHub monorepo.                 |
| `packslip:tool.example.com`                   | A signed release list hosted by the publisher. |
| `packslip:example.com/tools/mytool`           | One tool on a publisher's domain.              |

The project must publish packslip manifests. This backend does not guess
installation instructions from release filenames. GitHub projects have built-in
release discovery and signer identity rules; other hosts need a signed release
list and an explicit [signer configuration](#pubkey). For bundle filenames,
discovery URLs, and monorepo identity rules, see
[project discovery](/dev-tools/packslip-verification.html#project-discovery).

### Private GitHub repositories {#private-repositories}

A private repository works with the same
[GitHub credentials](/dev-tools/github-tokens.html) the `github:` backend uses,
for example `MISE_GITHUB_TOKEN`, a `github.credential_command`, or a token in
gh's `hosts.yml`. It needs nothing in your configuration:

```toml [mise.toml]
[tools]
"packslip:github.com/my-org/internal-cli" = "latest"
```

GitHub serves a private repository's release assets only through its API, not
from the `github.com/.../releases/download/...` URLs a manifest records, so when
a download fails mise retries through the API with your token. The token is used
only to transfer files. The signature, project identity, signer continuity, and
artifact digest checks do not change, and mise never reads a credential from the
signed manifest.

## Versions

List the available versions, or select one:

```sh
mise ls-remote packslip:github.com/jdx/hk
mise use packslip:github.com/jdx/hk@2.5.0
```

Only releases that carry a packslip manifest are listed. The registry shorthand
uses aqua for hk versions before 1.58.1, so `mise use hk@1.57.0` still works; you
can also ask for aqua directly with `mise use aqua:jdx/hk@1.57.0`.

The packslip format requires semantic versions, which include date versions such
as `2026.9.1`. Prereleases are excluded unless you enable the
[`prerelease`](#prerelease) tool option or the global
[`prereleases`](/configuration/settings.html#prereleases) setting.

[`minimum_release_age`](/configuration/settings.html#minimum_release_age) also
applies, so a release published within that window is not offered yet. An exact
version such as `hk@2.5.0`, or one recorded in `mise.lock`, installs during the
wait; only requests such as `latest` or `2` are held back. See
[Minimum release age](/security.html#minimum-release-age).

### How `latest` is selected {#latest}

`latest` follows the publisher's signed recommendation if there is one, then
GitHub's latest release, then the highest eligible version. Every candidate must
pass verification and your policies. See
[version resolution](/dev-tools/packslip-verification.html#version-resolution)
for withdrawals, fallbacks, and offline behavior.

### Reproduce an installation

A `latest` request can resolve to a newer release once the publisher recommends
one. To keep a team on the same release, generate a lockfile and commit it with
`mise.toml`:

```sh
mise lock
git add mise.toml mise.lock
```

[`mise lock`](/cli/lock.html) verifies the release and records, for each target
platform, the version, the artifact URL and checksum, the signer, and the forge
repository ID, without installing anything. Use `mise lock --platform` to choose
the platforms. Create the file this way: unless the
[`lockfile`](/configuration/settings.html#lockfile) setting is `true`,
`mise install` updates a `mise.lock` that already exists but does not create one.

Teammates and CI then install exactly what the lockfile records:

```sh
mise install --locked
```

Installation still needs the artifacts, or usable cached copies, and must pass
the current verification policy. See [Lockfile (mise.lock)](/dev-tools/mise-lock.html)
for target platforms and updates.

## Completions, man pages, and skills {#completions}

<span id="skills"></span><span id="resource-selection-and-command-execution"></span>

A release can declare shell completions, man pages, and agent skills, and they
follow the tool version active in each project. With
[mise activated](/shell-setup.html), type `hk` and press Tab to complete its
commands. For a tool that ships man pages, `man <tool>` opens the active
version's page. To link a tool's skills where your agent reads them, run
`mise skills sync --dir .agents/skills`. See
[Man pages, completions, and skills](/dev-tools/packslip-resources.html) for
setup without shell activation, automatic skill links, and when mise runs a
publisher's command.

## Verification {#what-is-verified}

Before unpacking a release, mise verifies the publisher's signature, the
requested project and version, and the selected download's digest and size. The
signer must match the identity the project name implies or the public key you
configured. mise also checks the signer it accepted before, any `mise.lock`
commitments, and the release-age and stamp policies that apply. It keeps the
verified manifest as `.mise-packslip.json` in the install directory.

Verification establishes who published the release and that the bytes are the
ones they signed. It does not establish that the software is safe. mise records
whether a manifest links build provenance, but it does not fetch or verify that
provenance. See [verification checks](/dev-tools/packslip-verification.html#verification-checks).

### Signer changes {#pinned-signers}

mise remembers the signer it accepted for each project in its local state, the
way SSH remembers hosts, and [`mise.lock`](/dev-tools/mise-lock.html) can carry
the same commitment to other machines. A release from a different signer is
refused. To see what this machine remembers:

```sh
mise packslip pins
mise packslip pins --json
```

When a project announces a new signing key or workflow:

1. Confirm the change in the publisher's release notes or announcement.
2. If you set `pubkey`, `identity`, `identity_prefix`, `workflow`, or
   `list_identity_prefix` for the tool, update them.
3. Reset this machine's pin:

   ```sh
   mise packslip forget github.com/jdx/hk
   ```

4. If `mise.lock` records the old signer, delete the tool's entries and run
   `mise lock` to record the new one.
5. Review and commit the lockfile change.

`mise packslip forget` also resets the project's remembered release-list state.
It does not change stamper-list state. See
[signer continuity](/dev-tools/packslip-verification.html#signer-continuity)
for the changes that are refused.

### Renamed repositories {#renamed-repositories}

mise pins GitHub and GitLab projects by the forge's repository ID, not only by
name. A renamed or transferred repository keeps installing, including releases
signed under the new name, and mise warns once with the new name; update your
configuration when convenient. The pin and the `mise.lock` commitment follow the
repository, so you do not need `mise packslip forget`.

A different repository that takes the old name is refused, because that is what
a deleted repository whose name someone else took looks like. The error names
the record that pins the original: this machine's pin, the `mise.lock` entries,
or both. Clear what it names (`mise packslip forget`, or delete the lockfile
entries) only after the project confirms it re-created the repository itself.

The first install on a machine with no pin and no lockfile entry trusts
whichever repository has the name at that moment, so commit `mise.lock`. See
[renamed, transferred, and re-created repositories](/dev-tools/packslip-verification.html#renamed-transferred-and-re-created-repositories)
for the details and limits.

## Tool options

These [tool options](/dev-tools/#tool-options) apply to one entry in `[tools]`.
The `packslip.exec`, `packslip.stampers`, and `skills.*` settings go under
`[settings]` instead.

| Option                                                                      | Default                         | Purpose                                                      |
| --------------------------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------ |
| [`variant`](#variant)                                                       | No variant                      | Select a publisher-declared alternative build.               |
| [`pubkey`](#pubkey)                                                         | Unset                           | Pin a minisign-format public key or public-key file.         |
| [`identity`, `identity_prefix`, `issuer`](#identity-identity-prefix-issuer) | Derived from a recognized forge | Set the expected keyless signer and OIDC issuer.             |
| [`workflow`](#workflow)                                                     | Any workflow of the repository  | Pin the repository workflow that signs releases on a tag.    |
| [`list_identity_prefix`](#list-identity-prefix)                             | Release signer policy           | Pin a different workflow for the vendor release list.        |
| [`prerelease`](#prerelease)                                                 | `false`                         | Include prerelease versions.                                 |
| [`trust`](#trust)                                                           | Apply configured stampers       | Use `"vendor"` to exempt this tool from stamp requirements.  |
| [`allow_unlogged`](#allow-unlogged)                                         | `false`                         | Accept key-signed bundles without transparency-log evidence. |
| [`ignore_requirements`](#ignore-requirements)                               | `false`                         | Install despite confirmed host requirement failures.         |

### `variant`

Select a named alternative build, such as `fips` or `baseline`. Without this
option, mise considers only artifacts that have no variant. The publisher must
provide the variant you request.

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", variant = "fips" }
```

### `pubkey`

For a key-signed project, get the publisher's public key through a channel you
trust. Set `pubkey` to the minisign-format public-key line or the path of its
`.pub` file. The release list and bundles must verify against that key.

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", pubkey = "/path/to/vendor.pub" }
```

### `identity`, `identity_prefix`, `issuer` {#identity-identity-prefix-issuer}

For keyless signing, set an exact certificate `identity` or an
`identity_prefix`, plus its OIDC `issuer`. These options override the policy
derived from the forge name. Keep the trailing slash in a repository prefix. A
domain project signed by a GitHub workflow could use:

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", identity_prefix = "https://github.com/example/tool/", issuer = "https://token.actions.githubusercontent.com" }
```

Replace the example identity with the one the publisher confirms. Recognizing
the signing issuer does not add release discovery: the domain still needs a
[signed release list](/dev-tools/packslip-verification.html#project-discovery).

### `workflow`

By default, mise accepts a release of a `github.com` project signed by any
workflow of its repository. Name the workflow file to accept only that workflow,
run on a tag:

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", workflow = "release.yaml" }
```

This expands to the identity prefix
`https://github.com/example/tool/.github/workflows/release.yaml@refs/tags/` with
GitHub's OIDC issuer, so a workflow run on a branch cannot sign a release. It
cannot be combined with `pubkey`, `identity`, `identity_prefix`, or `issuer`;
use those to pin another ref or forge. When the registry supplies a `workflow`
default and you set one of those options on the shorthand, yours replaces the
default. Like those options, it pins the signer by name, so a renamed repository
needs the new name here.

The same prefix also applies to the project's signed release list. If the
vendor signs that list from another workflow or ref, pin it with
[`list_identity_prefix`](/dev-tools/backends/packslip.html#list-identity-prefix).

### `list_identity_prefix` {#list-identity-prefix}

When a different workflow signs the vendor's release list, pin its certificate
identity prefix separately. It replaces `identity` and `identity_prefix` for the
vendor's list only; release bundles still need their original signer. The OIDC
`issuer` is shared, including an issuer derived from a forge project. Without
this option, the list uses the same policy as release bundles.

The value must be a non-empty string and requires an issuer. It cannot be
combined with `pubkey`, and it does not affect configured stampers, whose lists
use their own pins.

### `prerelease`

Include prereleases when listing or selecting versions:

```toml
[tools]
"packslip:github.com/jdx/hk" = { version = "latest", prerelease = true }
```

### `trust`

Set `trust = "vendor"` to exempt one tool from the configured
[stampers](/dev-tools/packslip-verification.html#stamps). The vendor's signature
is still verified, and mise records the choice in the lockfile options.

```toml
[tools]
"packslip:github.com/jdx/hk" = { version = "latest", trust = "vendor" }
```

### `allow_unlogged` {#allow-unlogged}

Set to `true` only when your policy accepts key-signed bundles without
transparency-log evidence. Signature and artifact verification still apply.

### `ignore_requirements` {#ignore-requirements}

Set to `true` to install despite a confirmed
[host requirement](/dev-tools/packslip-verification.html#host-requirements)
failure. mise then keeps the selected GNU build instead of falling back to musl.
This does not supply missing libraries or make an incompatible executable run,
and the other verification checks still apply.

## Policies {#advanced-policies}

<span id="signed-release-lists"></span><span id="release-list-continuity-and-minimum-age"></span><span id="stamps"></span><span id="artifact-selection"></span><span id="host-requirements"></span>

Publishers can recommend or withdraw versions in a signed release list, and you
can require approval from stampers you trust. The
[verification and policy reference](/dev-tools/packslip-verification.html)
covers [signed release lists](/dev-tools/packslip-verification.html#signed-release-lists),
[release-list continuity](/dev-tools/packslip-verification.html#release-list-continuity-and-minimum-age),
[stamps](/dev-tools/packslip-verification.html#stamps),
[artifact selection](/dev-tools/packslip-verification.html#artifact-selection),
and [host requirements](/dev-tools/packslip-verification.html#host-requirements).

## Troubleshooting

Before changing anything, check which backend and version are in use and which
signer mise remembers, then rerun the failing command with debug output:

```sh
mise tool hk
mise ls --current
mise packslip pins
MISE_DEBUG=1 mise install packslip:github.com/jdx/hk
```

Replace `hk` with the affected tool. `mise packslip pins` lists the identities
mise accepted before; it does not re-verify an installed executable or grant
trust to a new signer.

| Symptom                                       | Next step                                                                                                                                                                                               |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No versions or bundle found                   | Check the project identifier and `mise ls-remote`. Confirm that the release has a packslip manifest and is older than `minimum_release_age`. Only the publisher can add a missing manifest.             |
| Nothing pins the signer                       | Configure `pubkey`, or `identity` or `identity_prefix` with `issuer`, using details the publisher confirms.                                                                                             |
| Signer change or trust downgrade refused      | Compare `mise packslip pins`, the tool's options, and the `mise.lock` entry with the publisher's announcement, then follow [signer changes](#pinned-signers).                                           |
| A different repository under the same name    | Treat it as a possible takeover. See [renamed repositories](#renamed-repositories), and clear the pin or lockfile entries the error names only after the project confirms it re-created the repository. |
| Signed list expired, rolled back, or missing  | Ask the list's publisher or stamper for a current list. Removing local state would only discard the continuity check.                                                                                   |
| Release withdrawn or missing a required stamp | Select a version your stampers approve and the vendor has not withdrawn. See [stamps](/dev-tools/packslip-verification.html#stamps).                                                                    |
| Bundle or artifact digest or size mismatch    | Report the release and artifact to the publisher, or check your mirror. The download must match the signed manifest; do not accept new bytes to clear the error.                                        |
| No eligible artifact                          | Check your platform and requested `variant`. The publisher must provide a matching build.                                                                                                               |
| Ambiguous artifacts                           | The publisher must distinguish the builds in the manifest; no local option can choose between identically described artifacts.                                                                          |
| Host requirements failed                      | Install the reported dependency or use a compatible host. See [host requirements](/dev-tools/packslip-verification.html#host-requirements) before overriding a failure.                                 |
| 404 on a private repository's release         | Run `mise token github` to see which token mise selects, and confirm it can read the repository. See [private repositories](#private-repositories).                                                     |

Each error names the stage that failed. Changing an artifact option cannot
repair an invalid signature, and forgetting a signer pin cannot repair a digest
mismatch. For completion and skill errors, see
[resource troubleshooting](/dev-tools/packslip-resources.html#troubleshooting).

## Publishing tools for mise {#why-publish-one}

A packslip manifest lets mise install your releases without a new registry
shorthand or a filename-matching recipe. You can keep your release layout and
add versioned completions, CLI specifications, or agent skills.

To support mise:

1. Publish installable artifacts with accurate platform metadata and executable
   paths.
2. Sign a manifest that contains their digests and publish its bundle with the
   release.
3. For domain hosting, publish a signed release list and tell users how to pin
   your signer.
4. Optionally declare [completions, man pages, and skills](/dev-tools/packslip-resources.html).

For GitHub Actions, follow the
[packslip publishing guide](https://packslip.dev/docs/publishing/) for the
action version, permissions, inputs, and monorepo setup, and run the action
after building and uploading the final artifacts. For domain hosting, see
[signed release lists](https://packslip.dev/docs/release-lists/). The
[packslip specification](https://packslip.dev/release/v1/) defines the format.
