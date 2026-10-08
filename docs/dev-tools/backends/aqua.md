---
description: "Install prebuilt release binaries from aqua registry recipes, with signature and provenance checks."
---

# aqua backend

The `aqua` backend installs prebuilt release binaries using package recipes from
the [aqua registry](https://github.com/aquaproj/aqua-registry), for example
[`aqua:hashicorp/terraform`](https://github.com/aquaproj/aqua-registry/blob/main/pkgs/hashicorp/terraform/registry.yaml).
The backend works on Windows and can verify checksums, signatures, attestations
and SLSA provenance when a recipe declares them.

## Requirements

Nothing beyond mise. mise reads the recipes and runs the
[verification](#security-verification) itself, so you need neither the aqua CLI
nor the signing tools. The recipe must list your platform in `supported_envs`
(see [no asset for your platform](#no-asset-for-your-platform)) and download a
published artifact. A recipe that builds from source instead (type `cargo`,
`go_install` or `go_build`) fails with an error that points to the
[cargo](/dev-tools/backends/cargo.html) or [go](/dev-tools/backends/go.html)
backend.

## Usage

Install ripgrep in your project and run it without shell activation:

```sh
mise use aqua:BurntSushi/ripgrep
mise exec -- rg --version
```

This writes the following to `mise.toml`. Add `-g` to `mise use` for a global
tool.

```toml
[tools]
"aqua:BurntSushi/ripgrep" = "latest"
```

Run `mise ls-remote aqua:BurntSushi/ripgrep` to list available versions.
`mise registry ripgrep` shows the backends its short name uses.

Recipes download and extract published artifacts. A tool that needs custom
install steps or environment setup needs another backend.

## The bundled registry {#bundled-registry}

Each mise release bundles a snapshot of the aqua registry and uses it by
default. The opt-in [`registry_floating`](/configuration/settings.html#registry_floating)
setting checks the current official aqua registry first and keeps the bundled
snapshot as a fallback. It also floats mise's own registry; see
[floating registries](/registry.html#floating-registries) for the tradeoffs and
cache behavior.

If a recipe has wrong platform names, URLs or verification metadata, report it
to the [aqua registry](https://github.com/aquaproj/aqua-registry/issues). A fix
upstream reaches you through a later mise release, `registry_floating`, or a
custom registry.

## Custom registries {#custom-registry}

List your own aqua registries in [`aqua.registries`](/dev-tools/backends/aqua.html#aqua.registries). mise
checks them in order, then the bundled registry:

```toml
[settings]
aqua.registries = [
  "https://github.com/my-org/aqua-registry",
  "registry.yaml",
]
```

Each source can be a repository URL, a direct URL to a `registry.yaml` or
`registry.yml` file, an absolute `file://` URL, or, in a config file, a plain
path resolved against that config file's root, as `registry.yaml` is above.
Paths given through `MISE_AQUA_REGISTRIES` must be absolute `file://` URLs. For
a repository or directory, mise reads `registry.yaml` at its root, or
`registry.yml` if that is missing.

Remote sources are cached for
[`aqua.registry_cache_ttl`](/dev-tools/backends/aqua.html#aqua.registry_cache_ttl). Local sources are read
again each time mise loads the registry, so edits take effect immediately.

With [`aqua.baked_registry`](/dev-tools/backends/aqua.html#aqua.baked_registry) on (the default), the bundled
registry is the fallback for packages that none of your registries define. Aqua
registry aliases apply only inside the registry that defines them. To point a
mise name at a package in another registry, use a
[tool alias](/dev-tools/aliases.html).

[`aqua.registry_url`](/dev-tools/backends/aqua.html#aqua.registry_url) is deprecated; use `aqua.registries`.
mise 2026.12.0 and later warn when it is set, and mise 2027.12.0 removes it.
Until then it is read as a one-entry `aqua.registries` when `aqua.registries` is
unset.

## Security verification {#security-verification}

<span id="github-artifact-attestations"></span>
<span id="cosign-verification"></span>
<span id="slsa-provenance-verification"></span>
<span id="other-security-methods"></span>
<span id="verification-process"></span>

mise implements checksum, GitHub artifact attestation, Cosign, SLSA and
Minisign verification itself, so you do not need their CLI tools. Which checks
run depends on what each package's recipe declares.

| Method                       | Required publisher or registry metadata                                                                                   |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Checksums                    | An expected digest from registry metadata, a checksum file, the release API, or a lockfile.                               |
| GitHub artifact attestations | A registry attestation configuration identifying the expected workflow.                                                   |
| Cosign                       | A supported public-key or signature-bundle configuration; arbitrary Cosign CLI arguments are not executed.                |
| SLSA                         | A registry provenance configuration with `signer_identity` and `signer_issuer`, plus the publisher's provenance artifact. |
| Minisign                     | A signature and the expected public key.                                                                                  |

The `aqua.*` verification [settings](#settings) are on by default. Some checks
also have a global setting, such as
[`github_attestations`](/configuration/settings.html#github_attestations) or
[`slsa`](/configuration/settings.html#slsa).

The SLSA signer fields are the exact Fulcio certificate URI subject and OIDC
issuer. Packages whose recipe has no signer skip SLSA and may use another check.
To supply one, see
[`slsa_signer_identity` and `slsa_signer_issuer`](/dev-tools/backends/aqua.html#slsa-signer-identity-and-slsa-signer-issuer).

A verified [lockfile](/dev-tools/mise-lock.html) is trusted. When it records a
checksum and provenance, including SLSA, mise checks the artifact digest and
does not verify the provenance again, so installing from it needs no signer.
Set [`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance)
to verify provenance again during locked installs.

## Tool options

Set these on the tool's entry in `[tools]`. Options every backend accepts, such
as `install_env` and `depends`, are described under
[tool options](/dev-tools/#tool-options).

### `symlink_bins`

Some tools bundle executables you may not want on `PATH`. For example,
`aws-cli` bundles Python, which can shadow the Python you meant to use. With
`symlink_bins = true`, mise creates a `.mise-bins` directory that links only the
package's own executables, and puts that directory on `PATH` instead of every
executable in the install:

```toml
[tools]
"aqua:aws/aws-cli" = { version = "latest", symlink_bins = true }
```

The `aws-cli` registry short name already sets this option. mise links the
executables listed in the recipe's `files` field (`aws` and `aws_completer` for
aws-cli), or the package's main executable when the recipe lists none.

### `vars`

Some recipes use template variables, such as
<span v-pre>`{{.Vars.channel}}`</span>. Set them as top-level options or in a
nested `vars` table:

```toml
[tools]
"aqua:flutter/flutter" = { version = "3.32.8", channel = "stable" }
"aqua:scenarigo/scenarigo" = { version = "0.21.0", vars = { go_version = "1.24" } }
```

Variables with defaults are filled in automatically. A variable the recipe marks
as required must be set unless the recipe also gives it a default. A recipe
variable named `libc` must be set as `vars.libc`, because a top-level `libc` key
is the [`libc`](#libc) option.

### `libc`

On glibc Linux, mise installs a release's glibc build even when the recipe names
the musl build, and uses the musl build only when no glibc build is published.
Set `libc` to choose for one tool:

```toml
[tools]
"aqua:domcyrus/rustnet" = { version = "latest", libc = "musl" }
```

Accepted values are `glibc` (or `gnu`) and `musl`. When set, mise never falls
back to a build for the other libc, and the value overrides
[`libc = "glibc"`](/configuration/settings.html#libc) in settings. It cannot
select glibc on a musl platform: an Alpine host, `libc = "musl"` in settings, or
a `linux-*-musl` lockfile platform.

On a musl host where neither the tool nor settings ask for musl, mise installs
the asset the recipe names. If that is a glibc build, it needs glibc or a
compatibility layer such as gcompat to run.

The value is recorded in the lockfile, so changing it resolves the tool again.
Run `mise install --force` to switch a version that is already installed.
`latest` resolves with the option applied, but `mise ls-remote` lists versions
for the host's libc, because one version list serves every configuration of a
tool.

### `prerelease`

By default, releases marked as prereleases on GitHub are left out of
`mise ls-remote` and `latest`. Set `prerelease = true` to include them:

```toml
[tools]
"aqua:owner/tool" = { version = "latest", prerelease = true }
```

With it, prerelease tags such as `v1.0.0-rc1` appear in `mise ls-remote`, and
`latest` and prefix requests can match them. Draft releases are always
excluded. To include prereleases for every tool, set the
[`prereleases`](/configuration/settings.html#prereleases) setting.

For packages that use the `github_tag` version source, tags carry no prerelease
flag. `mise ls-remote` lists every tag, while `latest` and prefix requests skip
tags that look like prereleases (such as `-rc1` or `-beta`) unless
`prerelease = true` is set.

### `slsa_signer_identity` and `slsa_signer_issuer`

Some recipes configure `slsa_provenance` without the expected signer. mise then
skips SLSA for them, and an existing lockfile that requires SLSA fails. Set the
expected Fulcio certificate URI subject and OIDC issuer to supply the signer
yourself:

```toml
[tools."aqua:google/osv-scanner"]
version = "2.6.0"
slsa_signer_identity = "https://github.com/slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@refs/tags/v2.1.0"
slsa_signer_issuer = "https://token.actions.githubusercontent.com"
```

Set both options together. They must match the certificate exactly and replace
any signer in the recipe. The <span v-pre>`{{.Version}}`</span> template is
available in both values. Copy the signer from the project's release provenance,
not from an unreviewed source. These options only supply the signer: they do not
turn on SLSA for a package whose recipe has no `slsa_provenance`.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="aqua" :level="3" />

## Troubleshooting

<span id="common-aqua-issues"></span>

### Verification fails

Start with the failing command and its verification error:

```sh
MISE_DEBUG=1 mise install aqua:cli/cli
```

Check that the release publishes the expected signature or attestation, that the
recipe names the correct artifact and signer, and that your clock and network
allow certificate and transparency-log checks. For private assets or API rate
limits, set up [GitHub authentication](/dev-tools/github-tokens.html).

A digest mismatch means the artifact or the expected digest is wrong. A missing
or invalid signature means the publisher's release or the recipe's metadata is
wrong. Turning verification off changes which artifacts you trust; it does not
fix either problem. Report recipe problems to the
[aqua registry](https://github.com/aquaproj/aqua-registry/issues) and mise
problems to [mise issues](https://github.com/jdx/mise/issues), with credentials
removed.

### No asset for your platform

<span id="supported-env-missing"></span>

The recipe's `supported_envs` lists the platforms it covers. If the publisher
ships a build for your OS and architecture but the recipe leaves it out, report
it to the aqua registry. Until a fix lands, use the [github](/dev-tools/backends/github.html)
backend for that tool.

### Versions have an unexpected prefix

<span id="using-version-filter-instead-of-version-prefix"></span>

If `mise ls-remote aqua:...` shows versions such as `atlascli/1.2.3`, the recipe
filters tags with `version_filter` instead of stripping the prefix with
`version_prefix`. Report it to the aqua registry; mise strips and restores a
`version_prefix` automatically. Recipe authors can find these fixes in
[aqua registry recipes](/contributing/registry.html#aqua-registry-recipes).

Implementation: [`src/backend/aqua.rs`](https://github.com/jdx/mise/blob/main/src/backend/aqua.rs).
