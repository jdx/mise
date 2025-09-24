# Aqua Backend

[Aqua](https://aquaproj.github.io/) tools may be used natively in mise. aqua is the ideal backend
to use for new tools since they don't require plugins, they work on windows, they offer security
features in addition to checksums. aqua installs also show more progress bars, which is nice.

You do not need to separately install aqua. The aqua CLI is not used in mise at all. What is used is
the [aqua registry](https://github.com/aquaproj/aqua-registry) which is a bunch of yaml files that get compiled into the mise binary on release.
Here's an example of one of these files: [`aqua:hashicorp/terraform`](https://github.com/aquaproj/aqua-registry/blob/main/pkgs/hashicorp/terraform/registry.yaml).
mise has a reimplementation of aqua that knows how to work with these files to install tools.

As of this writing, aqua is relatively new to mise and because a lot of tools are being converted from
asdf to aqua, there may be some configuration in aqua tools that need to be tightened up. I put some
common issues below and would strongly recommend contributing changes back to the aqua registry if you
notice problems. The maintainer is super responsive and great to work with.

If all else fails, you can disable aqua entirely with [`MISE_DISABLE_BACKENDS=aqua`](/configuration/settings.html#disable_backends).

Currently aqua tools don't support setting environment variables or doing more than simply downloading
binaries though (and I'm not sure this functionality would ever get added), so some tools will likely
always require plugins like asdf/vfox.

The code for this is inside the mise repository at [`./src/backend/aqua.rs`](https://github.com/jdx/mise/blob/main/src/backend/aqua.rs).

## Usage

The following installs the latest version of ripgrep and sets it as the active version on PATH:

```sh
$ mise use -g aqua:BurntSushi/ripgrep
$ rg --version
ripgrep 14.1.1
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"aqua:BurntSushi/ripgrep" = "latest"
```

Some tools will default to use aqua if they're specified in [registry.toml](https://github.com/jdx/mise/blob/main/registry.toml)
to use the aqua backend. To see these tools, run `mise registry | grep aqua:`.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="aqua" :level="3" />

## Security Verification

Aqua backend supports multiple security verification methods to ensure the integrity and authenticity of downloaded tools. mise provides **native Rust implementation** for all verification methods, eliminating the need for external CLI tools like `cosign`, `slsa-verifier`, or `gh`.

### GitHub Artifact Attestations

GitHub Artifact Attestations provide cryptographic proof that artifacts were built by specific GitHub Actions workflows. mise verifies these attestations natively to ensure the authenticity and integrity of downloaded tools.

**Requirements:**

- The tool must have `github_artifact_attestations` configuration in the aqua registry for attestations to be verified
- No external tools required - verification is handled natively by mise

**Configuration:**

```bash
# Enable/disable GitHub attestations verification (default: true)
export MISE_AQUA_GITHUB_ATTESTATIONS=true
```

**Registry Configuration Example:**

```yaml
packages:
  - type: github_release
    repo_owner: cli
    repo_name: cli
    github_artifact_attestations:
      signer_workflow: cli/cli/.github/workflows/deployment.yml
```

### Cosign Verification

mise natively verifies Cosign signatures without requiring the `cosign` CLI tool to be installed.

**Configuration:**

```bash
# Enable/disable Cosign verification (default: true)
export MISE_AQUA_COSIGN=true

# Pass extra arguments to the verification process
export MISE_AQUA_COSIGN_EXTRA_ARGS="--key /path/to/key.pub"
```

### SLSA Provenance Verification

mise natively verifies SLSA (Supply-chain Levels for Software Artifacts) provenance without requiring the `slsa-verifier` CLI tool.

**Configuration:**

```bash
# Enable/disable SLSA verification (default: true)
export MISE_AQUA_SLSA=true
```

### Other Security Methods

Aqua also supports:

- **Minisign verification**: Uses minisign for signature verification
- **Checksum verification**: Verifies SHA256/SHA512/SHA1/MD5 checksums (always enabled)

### Verification Process

During tool installation, mise will:

1. Download the tool and any signature/attestation files
2. Perform native verification using the configured methods
3. Display verification status with progress indicators
4. Abort installation if any verification fails

**Example output during installation:**

```
✓ Downloaded cli/cli v2.50.0
✓ GitHub attestations verified
✓ Tool installed successfully
```

### Troubleshooting

If verification fails:

1. **Check network connectivity**: Verification requires downloading attestation data
2. **Verify tool configuration**: Ensure the aqua registry has correct verification settings
3. **Disable specific verification**: Temporarily disable problematic verification methods
4. **Enable debug logging**: Use `MISE_DEBUG=1` to see detailed verification logs

**Common issues:**

- **No attestations found**: The tool may not have attestations configured in the registry
- **Verification timeout**: Network issues or slow attestation services
- **Certificate validation**: Clock skew or certificate chain issues

To disable all verification temporarily:

```bash
export MISE_AQUA_GITHUB_ATTESTATIONS=false
export MISE_AQUA_COSIGN=false
export MISE_AQUA_SLSA=false
export MISE_AQUA_MINISIGN=false
```

## Common aqua issues

Here's some common issues I've seen when working with aqua tools.

### Supported env missing

The aqua registry defines supported envs for each tool of the os/arch. I've noticed some of these
are simply missing os/arch combos that are in fact supported—possibly because it was added after
the registry was created for that tool.

The fix is simple, just edit the `supported_envs` section of `registry.yaml` for the tool in question.

### Using `version_filter` instead of `version_prefix`

This is a weird one that causes weird issues in mise. In general in mise we like versions like
`1.2.3` with no decoration like `v1.2.3` or `cli-v1.2.3`. This consistency not only makes `mise.toml`
cleaner but, it also helps make things like `mise up` function right because it's able to parse it as
semver without dealing with a bunch of edge-cases.

Really if you notice aqua tools are giving you versions that aren't simple triplets, it's worth fixing.

One common thing I've seen is registries using a `version_filter` expression like `Version startsWith "Version startsWith "atlascli/""`.

This ultimately causes the version to be `atlascli/1.2.3` which is not what we want. The fix is to use
`version_prefix` instead of `version_filter` and just put the prefix in the `version_prefix` field.
In this example, it would be `atlascli/`. mise will automatically strip this out and add it back in,
which it can't do with `version_filter`.
