# Aqua Backend

[Aqua](https://aquaproj.github.io/) tools may be used natively in mise. aqua is the ideal backend
to use for new tools since they don't require plugins, they work on windows, they offer security
features like cosign/slsa verification in addition to checksums. aqua installs also show more progress
bars, which is nice.

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

## Common aqua issues

Here's some common issues I've seen when working with aqua tools.

### Supported env missing

The aqua registry defines supported envs for each tool of the os/arch. I've noticed some of these
are simply missing os/arch combos that are in fact supported—possibly because it was added after
the registry was created for that tool.

The fix is simple, just edit the `supported_envs` section of `registry.yaml` for the tool in question.

### Using `version_filter` instead of `version_prefix`

This is a weird one that doesn't causes weird issues in mise. In general in mise we like versions like
`1.2.3` with no decoration like `v1.2.3` or `cli-v1.2.3`. This consistency not only makes `mise.toml`
cleaner but, it also helps make things like `mise up` function right because it's able to parse it as
semver without dealing with a bunch of edge-cases.

Really if you notice aqua tools are giving you versions that aren't simple triplets, it's worth fixing.

One common thing I've seen is registries using a `version_filter` expression like `Version startsWith "Version startsWith "atlascli/""`.

This ultimately causes the version to be `atlascli/1.2.3` which is not what we want. The fix is to use
`version_prefix` instead of `version_filter` and just put the prefix in the `version_prefix` field.
In this example, it would be `atlascli/`. mise will automatically strip this out and add it back in,
which it can't do with `version_filter`.
