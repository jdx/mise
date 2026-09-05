# Packslip Backend

The [Packslip](https://packslip.dev) backend installs tools using signed release
manifests published by their maintainers. mise verifies the release, selects the
build for your platform, and installs its executables. Releases can also include
shell completions and agent skills.

Packslip is the preferred [Tier 1 backend](/registry.html#backends) for tools
whose publishers provide these manifests. For other tools, use
[aqua](/dev-tools/backends/aqua.html), [github](/dev-tools/backends/github.html),
or another supported backend. You do not need to install the Packslip CLI.

## Quick start {#usage}

With [mise activated](/getting-started.html), install [hk](https://hk.jdx.dev),
a git hook and lint manager, in your project:

```sh
mise use packslip:github.com/jdx/hk
hk --version
```

This records hk in the project's `mise.toml`. The equivalent configuration is:

```toml
[tools]
"packslip:github.com/jdx/hk" = "latest"
```

Run `mise install` after adding that configuration manually. To make hk
available outside a project, use `mise use -g packslip:github.com/jdx/hk`.
The registry shorthand `mise use hk` also selects Packslip by default.

## Supported project identifiers {#project-names-and-discovery}

Use `packslip:` followed by the project's host and path, without `https://`.
For GitHub, you can omit the host: `packslip:jdx/hk` is equivalent to
`packslip:github.com/jdx/hk`.

| Identifier                                    | Source                                         |
| --------------------------------------------- | ---------------------------------------------- |
| `packslip:github.com/owner/repo`              | A GitHub repository's releases.                |
| `packslip:github.com/owner/repo/tools/mytool` | One tool in a GitHub monorepo.                 |
| `packslip:tool.example.com`                   | A signed release list hosted by the publisher. |
| `packslip:example.com/tools/mytool`           | One tool on a publisher's domain.              |

The project must publish Packslip manifests; this backend does not infer
installation instructions from arbitrary release filenames. GitHub projects
have built-in release discovery and signer identity rules. Other hosts need a
signed release list and an explicit [signer configuration](#pubkey).

For bundle filenames, discovery URLs, and monorepo identity rules, see
[project discovery](/dev-tools/packslip-verification.html#project-discovery).

## Versions

List available versions or select a specific release:

```sh
mise ls-remote packslip:github.com/jdx/hk
mise use packslip:github.com/jdx/hk@1.58.1
```

Only releases with Packslip manifests are available through this backend.
The registry shorthand selects Aqua for hk versions before 1.58.1, so
`mise use hk@1.57.0` continues to work. You can also select Aqua explicitly
with `mise use aqua:jdx/hk@VERSION`.

Packslip uses semantic versions, including compatible date versions such as
`2026.9.1`. Prereleases are excluded unless you enable the
[`prerelease`](#prerelease) tool option. mise also applies
[`minimum_release_age`](/configuration/settings.html#minimum_release_age),
which defaults to 24 hours. A recently published release may therefore be
absent from the list until it reaches that age.

### How `latest` is selected {#latest}

`latest` follows the publisher's recommendation when it is eligible:

1. The publisher's `latest` pointer in a signed release list.
2. GitHub's latest release, if there is no signed pointer.
3. The highest eligible semantic version, if there is no eligible recommendation.

A publisher can recommend an older supported release even when a newer major
version exists. Prefix and channel requests keep their normal matching rules.
Every candidate must satisfy the configured verification and installation policies.

Offline, version listing and `latest` use cached results, or return no versions
if the cache is empty. Installation still performs verification. See
[version resolution](/dev-tools/packslip-verification.html#version-resolution)
for signed lists, withdrawals, and fallback behavior.

## Completions and skills {#completions}

hk publishes a [usage](https://usage.jdx.dev) CLI specification that mise can use
for shell completions. Install `usage`, then set up completions for your shell:

```sh
mise use -g usage
mise completion zsh --tool hk --install
```

Follow any shell setup instructions printed by the command. The installed
completion file follows the hk version active in each project. Bash, fish, and
PowerShell are also supported.

<span id="skills"></span>

Tools can also publish agent skills. Use `mise skills ls` to see the skills
provided by active tools and `mise skills sync --dir .agents/skills` to link them
into your agent's directory. A tool must declare a skill for it to appear.

<span id="resource-selection-and-command-execution"></span>

See [Packslip completions and skills](/dev-tools/packslip-resources.html) for
setup, automatic skill synchronization, and when resource generation runs a
publisher's executable.

## Verification {#what-is-verified}

mise verifies the publisher's signature, the requested project and version,
and the selected download's digest and size before unpacking it. The signer
must match the expected repository identity or your configured public key.
mise also checks any existing signer pin, lockfile commitments, and applicable
release-age or stamper policy.

A **manifest** describes the release. A **bundle** contains the manifest and its
signature evidence. An **artifact** is a downloadable build named by the
manifest. mise retains the verified manifest as `.mise-packslip.json` in the
install directory.

Verification authenticates the publisher and downloaded bytes. It does not
establish that the software is safe. mise records whether build provenance
links are present, but does not fetch and verify that linked provenance.
See [verification details](/dev-tools/packslip-verification.html#verification-checks).

### Signer changes {#pinned-signers}

mise remembers previously accepted signers in its local state.
[`mise.lock`](/dev-tools/mise-lock.html) can also record the project's signer
and artifact checksums, carrying those commitments to another machine.

If a release changes signer, inspect the remembered identity:

```sh
mise packslip pins
mise packslip pins --json
```

After confirming the publisher's announced signing-key or workflow change,
reset the local pin for that project:

```sh
mise packslip forget github.com/jdx/hk
```

This also resets the remembered vendor release-list continuity. It does **not**
change explicit signer options, erase stamper-list state, or remove a signer
commitment from `mise.lock`. Configure the new signer policy first; if a lockfile
entry conflicts, remove that entry and regenerate it with `mise install`.
Review and commit the resulting lockfile change. See
[signer continuity](/dev-tools/packslip-verification.html#signer-continuity)
for the changes that require this review.

## Tool options

These [tool options](/dev-tools/#tool-options) apply to one entry in `[tools]`.
The settings `packslip.exec`, `packslip.stampers`, and `skills.*` belong under
`[settings]` instead.

| Option                                                                      | Default                         | Purpose                                                      |
| --------------------------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------ |
| [`variant`](#variant)                                                       | No variant                      | Select a publisher-declared alternative build.               |
| [`pubkey`](#pubkey)                                                         | Unset                           | Pin a minisign-format public key or public-key file.         |
| [`identity`, `identity_prefix`, `issuer`](#identity-identity-prefix-issuer) | Derived from a recognized forge | Set the expected keyless signer and OIDC issuer.             |
| [`list_identity_prefix`](#list-identity-prefix)                             | Release signer policy           | Pin a different workflow for the vendor release list.        |
| [`prerelease`](#prerelease)                                                 | `false`                         | Include prerelease versions.                                 |
| [`trust`](#trust)                                                           | Apply configured stampers       | Use `"vendor"` to exempt this tool from stamp requirements.  |
| [`allow_unlogged`](#allow-unlogged)                                         | `false`                         | Accept key-signed bundles without transparency-log evidence. |
| [`ignore_requirements`](#ignore-requirements)                               | `false`                         | Install despite confirmed host requirement failures.         |

### `variant`

Select a named alternative build, such as `fips` or `baseline`. Without this
option, mise considers only artifacts with no variant. The publisher must
provide the requested variant.

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", variant = "fips" }
```

### `pubkey`

For a key-signed project, obtain the publisher's public key through a trusted
channel. Set `pubkey` to the minisign-format public-key line or the path to its
`.pub` file. The release list and bundles must verify against that key.

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", pubkey = "/path/to/vendor.pub" }
```

### `identity`, `identity_prefix`, `issuer` {#identity-identity-prefix-issuer}

For keyless signing, specify an exact certificate `identity` or an
`identity_prefix`, plus its OIDC `issuer`. These options override the policy
derived from the forge name. Keep the trailing slash in a repository prefix.
For example, a domain project signed by a GitHub workflow could use:

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", identity_prefix = "https://github.com/example/tool/", issuer = "https://token.actions.githubusercontent.com" }
```

Replace the example identity with the publisher's verified identity. Recognizing
the signing issuer does not add release discovery: the domain still needs a
[signed release list](/dev-tools/packslip-verification.html#project-discovery).

### `list_identity_prefix` {#list-identity-prefix}

When a different workflow signs the vendor's release list, pin its certificate
identity prefix separately. It replaces `identity` and `identity_prefix` only
for the vendor's list; release bundles still require their original signer.
The OIDC `issuer` is shared (including an issuer derived from a forge project).
Without this option, the list uses the same policy as release bundles.

The value must be a non-empty string and requires an issuer. It cannot be
combined with `pubkey`. It does not affect configured stampers, whose lists
use their own pins.

### `prerelease`

Include prereleases when listing or selecting versions:

```toml
[tools]
"packslip:github.com/jdx/hk" = { version = "latest", prerelease = true }
```

### `trust`

Use `trust = "vendor"` to exempt one tool from configured
[stampers](#stamps). Vendor signature verification still applies, and mise
records this choice in the lockfile options.

```toml
[tools]
"packslip:github.com/jdx/hk" = { version = "latest", trust = "vendor" }
```

### `allow_unlogged` {#allow-unlogged}

Set to `true` only when your policy accepts key-signed bundles without
transparency-log evidence. Signature and artifact verification still apply.

### `ignore_requirements` {#ignore-requirements}

Set to `true` to install despite confirmed [host requirement](#host-requirements)
failures. This does not supply missing libraries or make an incompatible
executable run. Other verification checks still apply.

## Advanced policies

### Signed release lists

Publishers can use signed lists to recommend or withdraw versions and provide
bundle locations. For domain projects, a list is required; for GitHub, it
supplements release discovery. See
[signed release lists](/dev-tools/packslip-verification.html#signed-release-lists).

<span id="release-list-continuity-and-minimum-age"></span>

Accepted lists must remain available and current. See
[list continuity and release age](/dev-tools/packslip-verification.html#release-list-continuity-and-minimum-age)
for expiry, rollback protection, and timestamp checks.

### Stamps

Stampers are services you configure to approve releases in addition to the
publisher's signature. No stamps are required by default. See
[stamper configuration and mirrors](/dev-tools/packslip-verification.html#stamps).

### Artifact selection and host requirements {#artifact-selection}

<span id="host-requirements"></span>

mise selects a build using the signed OS, architecture, libc, and variant
metadata, then checks its declared host requirements. An ambiguous build or a
confirmed incompatible host can prevent installation. See
[artifact selection](/dev-tools/packslip-verification.html#artifact-selection)
and [host requirements](/dev-tools/packslip-verification.html#host-requirements).

## Troubleshooting

Start with debug output for the failing command, for example:

```sh
MISE_DEBUG=1 mise install packslip:github.com/jdx/hk
```

| Symptom                                      | Next step                                                                                                                                                                                     |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No versions or bundle found                  | Check the project identifier and `mise ls-remote`. Confirm that the release has a Packslip manifest and is old enough for `minimum_release_age`. The publisher must supply missing manifests. |
| Nothing pins the signer                      | Configure `pubkey`, or a certificate identity/prefix and issuer, using details confirmed with the publisher.                                                                                  |
| Signer change or trust downgrade refused     | Inspect `mise packslip pins`, explicit tool options, and `mise.lock`. Follow [signer changes](#pinned-signers) after confirming the publisher's change.                                       |
| No eligible artifact                         | Check your platform and requested variant. The publisher must provide a matching build.                                                                                                       |
| Ambiguous artifacts                          | The publisher must distinguish the builds in the manifest; changing local options cannot fix identical metadata.                                                                              |
| Host requirements failed                     | Install the reported dependency or use a compatible host. See [host requirements](#host-requirements) before overriding a failure.                                                            |
| Signed list expired, rolled back, or missing | Ask the list's publisher for a current valid list. Removing an accepted list does not reset its policy.                                                                                       |
| Version excluded by stamp policy             | Check your configured stampers. A trusted stamper must approve the version, and the vendor must not have withdrawn it.                                                                        |
| Digest or size mismatch                      | Report the affected release and artifact to the publisher; the download must match the signed manifest.                                                                                       |

For completion and skill errors, see
[resource troubleshooting](/dev-tools/packslip-resources.html#troubleshooting).

## Publishing tools for mise {#why-publish-one}

A Packslip manifest lets mise install your releases without a new registry
shorthand or a separate filename-matching recipe. You can keep your existing
release layout and add versioned completions, CLI specifications, or agent skills.

To support mise:

1. Publish installable artifacts with accurate platform metadata and executable paths.
2. Sign a manifest containing their digests and publish its bundle with the release.
3. For domain hosting, publish a signed release list and tell users how to pin your signer.
4. Optionally declare [completions and skills](/dev-tools/packslip-resources.html).

For GitHub Actions, follow the [Packslip publishing guide](https://packslip.dev/docs/publishing/)
for the action version, permissions, inputs, and monorepo setup. Run the action
after building and uploading the final artifacts. For domain hosting, see
[signed release lists](https://packslip.dev/docs/release-lists/).

The [Packslip specification](https://packslip.dev/release/v1/) defines the format.
For mise's implementation details, see
[`src/backend/packslip.rs`](https://github.com/jdx/mise/blob/main/src/backend/packslip.rs).
