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

## mise plugins

Plugins are by far the biggest source of potential problems and where the most work still needs to be made.

There are 3 types of plugins:

- [core](https://github.com/jdx/mise/issues/236) - plugins that are hardcoded into the CLI.
  These are official plugins for the most common languages written in Rust.
- community - plugins in the [mise-plugins](https://github.com/mise-plugins) GitHub Org. [See below](#mise-plugins-github-org) for details.
- external - plugins owned by other parties, these include plugins in the shorthand registry. These are no more
  secure than installing any random tool from the internet. These receive a warning dialog when installed in mise.

Just because a plugin is inside of the shorthand registry (so you can run `mise install foo@`, does not mean
I vouch for it. I have no idea who almost anyone that builds those plugins are. If it's coming from the mise-plugins
GitHub org, you can have more trust in it. (See the owners with `mise plugins ls-remote --urls`).

Over time we should be able to move more plugins into being fully maintained by mise. I plan to add an
`MISE_PARANOID=1` env var that, when set, will make changes to make mise behave as securely as possible
(e.g.: only using core/mise-plugins plugins, only allowing plugins that use GPG verification of assets).

## [mise-plugins](https://github.com/mise-plugins) GitHub org

This is similar to <https://github.com/asdf-community> but with the advantage of being more secure by keeping the
contributor count minimal—currently only @jdx will be allowed to merge PRs. For this reason, plugins using this
organization will not receive a confirmation warning dialog when installed with mise as they've been vetted by a
trusted source.

If you're a plugin maintainer that would like to move your repo to this org [please let me know](https://github.com/orgs/mise-plugins/discussions).
Plugins can either retain compatibility with asdf or use mise specific functionality—that's up to you. asdf-compatible plugins should use "asdf-" as the prefix and "mise-" prefixed-plugins denote mise-only compatibility.

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
