---
description: "Look up the short tool names mise knows and the backends each one installs from."
editLink: false
---

# Registry

<script setup>
import Registry from '/components/registry.vue';
</script>

The registry maps short tool names such as `jq` or `aws-cli` to the backends
that install them, so `mise use jq` works without a full backend identifier.
Search the [tool list](#tools) below, or ask the mise you have installed.

The registry lives in
[`registry/`](https://github.com/jdx/mise/tree/main/registry) in the mise
repository, and each mise release bundles a copy of it. A tool without a
shorthand can still be installed by its [backend identifier](/dev-tools/backends/),
such as `github:owner/repo`, if the backend supports that project's release
layout or package format.

## Inspect a shorthand

```sh
mise registry           # every shorthand and its backends, preferred first
mise registry aws-cli   # aqua:aws/aws-cli asdf:MetricMike/asdf-awscli
mise search jq          # fuzzy-match names and show descriptions
mise tool aws-cli       # the backend and options mise uses for this tool
```

[`mise registry`](/cli/registry.html) lists aliases as their own rows, and
`mise registry --json --security` adds the signature and provenance checks of
each tool's backends. [`mise search`](/cli/search.html) prints each match with
its description. Run [`mise use`](/cli/use.html) with no arguments to pick a tool
interactively.

## Options a shorthand sets

A shorthand can set backend options as well as a backend. `aws-cli` installs
through `aqua:aws/aws-cli` with `symlink_bins = true`. If you write
`aqua:aws/aws-cli` yourself, that option is not applied and the installation
differs. `mise tool aws-cli` shows the options a shorthand applies.

## Which backend a shorthand uses

mise uses the first backend in the shorthand's list that is enabled, supports
your platform and covers the requested version. A backend can declare a
`min_version` and an exclusive `max_version`. hk's packslip backend starts at
1.58.1, so `hk@1.58.1` and later install with packslip while older versions use
aqua. A lock entry, an installed plugin, a
[`[tool_alias]`](/dev-tools/aliases.html) or a `MISE_BACKENDS_<TOOL>`
environment variable can override that choice. Run `mise tool <name>` to see
the backend mise uses, and see
[how backend selection works](/dev-tools/backends/#how-backend-selection-works)
for the full order.

## Floating registries

By default, mise uses the mise and aqua registry snapshots that were tested and
bundled with its release. If your system package manager ships mise updates
slowly, you can use current registry data without replacing mise:

```sh
mise settings registry_floating=true
```

With [`registry_floating`](/configuration/settings.html#registry_floating) on,
mise fetches the shorthand registry published with the latest mise release and
the current official aqua registry. The bundled snapshots remain the fallback
when a remote registry cannot be loaded. Fast and offline commands never
refresh the mise registry; they use the cached copy or the bundled snapshot.
The mise registry is cached for
[`registry_cache_ttl`](/configuration/settings.html#registry_cache_ttl) and the
aqua registry for
[`aqua.registry_cache_ttl`](/configuration/settings.html#aqua.registry_cache_ttl).
`mise cache clear` makes mise download both again the next time it is online.

A floating registry can contain changes made after your mise version was
tested, which is why it is off by default. Updating mise is still the better
choice when an updated package is available.

## Adding a tool to the registry {#backends}

The registry is curated: new shorthands are for widely used tools, and some
backends are preferred over others. Read
[Adding tools to the registry](/contributing/registry.html#backend-acceptance-tiers)
before you submit one. You do not need a shorthand to use a tool; write its
backend identifier instead, such as `github:owner/repo` or `cargo:name`.

## Tools {#tools}

Badges next to a backend show the signature or provenance checks mise performs,
in addition to checksums, when it installs through that backend:

| Badge                                | Check                                                                                                                                                      |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| packslip                             | A [signed release manifest](/dev-tools/backends/packslip.html), verified against the project's pinned signer                                               |
| attestations, SLSA, cosign, minisign | [aqua verification](/dev-tools/backends/aqua.html#security-verification) that the bundled aqua registry configures for at least one version of the package |

aqua badges come from registry metadata, so a check may apply only to some
versions or platforms. Other backends, such as `github:`, detect what a release
publishes at install time and are not badged here. When
[mise-versions.jdx.dev](https://mise-versions.jdx.dev/) tracks a tool, its
details link opens the tool's page there, with its versions and the security
information mise reports. On the command line, `mise tool <name>` reports the
checks for the backend mise would use, while `mise registry --json --security`
merges the checks of every backend registered for a tool.

<Registry />
