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

## Cosign and slsa verification

mise will verify signatures of tools using [cosign](https://docs.sigstore.dev/) and [slsa-verifier](https://github.com/slsa-framework/slsa-verifier)
if cosign/slsa-verifier is installed and the tool is configured to support it. Typically, these will be tools using aqua as the backend.
See the [aqua docs](https://aquaproj.github.io/docs/reference/security/cosign-slsa) for more on how this is
configured in the [aqua registry](https://github.com/aquaproj/aqua-registry).

## `mise.lock`

mise has support for [lockfiles](https://mise.jdx.dev/configuration/settings.html#lockfile) which will
store/verify the checksum of tool tarballs. Committing this into your repository is a good way to ensure
that the exact same version of a tool is installed across all developers and CI/CD systems.

Not all backends support this—notably asdf plugins do not.

## asdf plugins

asdf plugins are by far the biggest source of potential problems since they are typically not written
by the tool vendor and do not have checksum or signature verification—or if they do it isn't tied into
mise lockfiles.

I'm actively moving away from using asdf plugins where possible towards backends like aqua and ubi.
This has the added benefit of supporting Windows if the tool itself supports it.
If a tool uses an asdf plugin you will receive a prompt in mise before installing it to check the plugin's source code.

Please contribute to this effort by checking if a tool works in ubi or aqua and submitting a PR to
[registry.toml](https://github.com/jdx/mise/blob/main/registry.toml) to add it. If it doesn't work
in ubi or is missing from aqua, submit an issue or PR to the respective project to add it. New tools
using asdf are not likely to be accepted unless they cannot be supported in any other way.

## Supported Versions

The only supported version is the most recent one.

## Reporting a Vulnerability

Send an email to security@<MISE_JDX_DOMAIN_IN_README>

If you want, you may encrypt the message with GPG:

<details>
  <summary>@jdx's public key</summary>

```
-----BEGIN PGP PUBLIC KEY BLOCK-----

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
1wxfuzN2+ao=
=/CHf
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
