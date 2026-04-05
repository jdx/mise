# npm Backend

You may install packages directly from [npmjs.org](https://npmjs.org/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).

## Dependencies

This relies on having `npm` installed for resolving package versions.
If you use `bun` or `pnpm` as the package manager, they must also be installed.

When [`install_before`](/configuration/settings.html#install_before) is set, the npm backend
forwards that cutoff to transitive dependency resolution during install. This relies on the
configured package manager supporting its native release-age flag:

- `npm >= 6.9.0` using `--before <timestamp>` (`Node >= 10.16.0` if you rely on bundled npm)
- `bun >= 1.3.0` using `--minimum-release-age <seconds>`
- `pnpm >= 10.16.0` using `--config.minimumReleaseAge=<minutes>`

If you want transitive protection, install and use a package manager version that meets the
corresponding requirement above. Older versions may fail while processing the forwarded argument.

Here is how to install `npm` with mise:

```sh
mise use -g node
```

To install `bun` or `pnpm`:

```sh
mise use -g bun
# or
mise use -g pnpm
```

## Usage

The following installs the latest version of [prettier](https://www.npmjs.com/package/prettier)
and sets it as the active version on PATH:

```sh
$ mise use -g npm:prettier
$ prettier --version
3.1.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"npm:prettier" = "latest"
```

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="npm" :level="3" />
