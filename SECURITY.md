# Security Policy

mise is a convenient tool to manage developer tools, however its model is also open to potential risks. The following
are major areas of mise and the security considerations currently being made and what needs to be made in the future.

Please open a ticket or send me an email if you have thoughts on how mise can be made more secure.

## Core CLI Security

Development of the "core CLI" is done on jdx/mise which only a single developer (me, @jdx) has access to.
Other contributors may only submit contributions via public Pull Requests. Reducing the number
of developers with access down to 1 minimizes the chance of keys being leaked.

This does create a [bus factor](https://en.wikipedia.org/wiki/Bus_factor) problem. If I suddenly died one day
or otherwise wasn't able to continue development at all there are some successors listed in my GitHub account
that can take over my account if need be.

The dependencies in the core CLI are a security vector. I've tried to be judicious about what dependencies make it into
the project. I only select dependencies with broad usage across the Rust community where possible.
I'm open to PRs or suggestions on reducing dependency count even at the cost of functionality because it will make
mise more secure.

## mise.jdx.dev

mise.jdx.dev is the asset host for mise. It's used to host precompiled mise CLI binaries, and hosts a "[VERSION](https://mise.jdx.dev/VERSION)"
which mise uses to occasionally check for a new version being released. Everything hosted there uses a single
vendor to reduce surface area.

## Native Security Verification

mise provides **native Rust implementation** for security verification of tools, eliminating the need for external dependencies like `cosign`, `slsa-verifier`, `minisign`, or `gh` CLI tools. This applies to tools using the aqua backend.

### Supported Verification Methods

- **Cosign signatures**: Keyless and key-based signature verification
- **SLSA provenance**: Verification of Supply-chain Levels for Software Artifacts (SLSA) attestations
- **GitHub Artifact Attestations**: Verification of GitHub's artifact attestation system
- **Minisign verification**: Verification of minisign signatures
- **Checksum verification**: Always enabled for supported backends

### Configuration

All verification methods are enabled by default and can be configured via environment variables:

```bash
# Enable/disable specific verification methods
export MISE_AQUA_COSIGN=true                 # Default: true
export MISE_AQUA_SLSA=true                   # Default: true
export MISE_AQUA_GITHUB_ATTESTATIONS=true    # Default: true
export MISE_AQUA_MINISIGN=true               # Default: true
```

### How it Works

You will see this verification happen automatically when aqua tools are installed. The verification status is displayed during installation with progress indicators. If any verification fails, the installation will be aborted.

See the [aqua docs](https://aquaproj.github.io/docs/reference/security/cosign-slsa) for more on how verification is configured in the [aqua registry](https://github.com/aquaproj/aqua-registry).

If you notice a tool offers security verification methods (gpg/slsa/cosign/minisign/etc), consider making a PR to the aqua registry to enable verification for that tool.

## `mise.lock`

mise has support for [lockfiles](https://mise.en.dev/configuration/settings.html#lockfile) which will
store/verify the checksum of tool tarballs. Committing this into your repository is a good way to ensure
that the exact same version of a tool is installed across all developers and CI/CD systems.

Not all backends support this—notably asdf plugins do not.

## asdf plugins

asdf plugins in asdf (but not with mise's default tools) are dangerous. They are typically owned by random developers
unconnected to either asdf or the tool vendor. They may get hacked or maliciously inject code into
their plugin that could trivially execute code on your machine.

asdf plugins are not used for tools inside the [registry](https://github.com/jdx/mise/blob/main/registry/) whenever possible.
Sometimes it is not possible to use more secure backends like aqua/ubi because tools have complex install
setups or need to export env vars. As of 2025-01-08, <25% of tools use asdf plugins as the default backend.
All of these are hosted in the [mise-plugins org](https://github.com/mise-plugins) to secure the supply
chain so you do not need to rely on plugins maintained by anyone except me.

Of course if you _manually_ add plugins not from the mise-plugins org you will want to ensure they
are coming from a trusted source.

Please contribute to this effort to migrate away from asdf plugins by checking if a tool works in ubi or aqua and submitting a PR to
[registry/](https://github.com/jdx/mise/blob/main/registry/) to add it. If it doesn't work
in ubi or is missing from aqua, submit an issue or PR to the respective project to add it. New tools
using asdf are **not** likely to be accepted unless they cannot be supported with any other backend.

## Supported Versions

The only supported version is the most recent one.

## Reporting a Vulnerability

Send an email to security@<MISE_JDX_DOMAIN_IN_README>

If you want, you may encrypt the message with GPG:

<details>
  <summary>mise security reports public key</summary>

Fingerprint: `70B4 0C16 536D 6FCE A883 F865 335E D915 E315 E085`

```
-----BEGIN PGP PUBLIC KEY BLOCK-----

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
=EGaQ
-----END PGP PUBLIC KEY BLOCK-----
```

</details>

## Release gpg key

This is the gpg key used to sign deb releases and the SHASUMS files
contained within releases.

<details>
  <summary>Release gpg key</summary>

```
-----BEGIN PGP PUBLIC KEY BLOCK-----
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
S1GIqgTqW7TRaQRg
=yIM2
-----END PGP PUBLIC KEY BLOCK-----
```

</details>
