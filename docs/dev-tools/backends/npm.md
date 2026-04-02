# npm Backend

You may install packages directly from [npmjs.org](https://npmjs.org/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).

## Dependencies

This relies on having `npm` installed for resolving package versions.
If you use `bun` or `pnpm` as the package manager, they must also be installed.

When [`install_before`](/configuration/settings.html#install_before) is set, the npm backend
enforces that cutoff in two places during install:

- the selected top-level `npm:` package version must have been released before the cutoff, even if
  it was specified explicitly
- transitive dependency resolution also receives the same cutoff

The top-level package cutoff is enforced by mise itself. The transitive dependency protection relies
on the configured package manager supporting its native release-age flag:

- `npm`: `--before <timestamp>`
- `bun`: `--minimum-release-age <seconds>`
- `pnpm`: `--config.minimumReleaseAge=<minutes>`

If you want transitive protection, install and use an `npm`/`bun`/`pnpm` version that supports the
corresponding flag. Otherwise the package manager may fail while processing the forwarded argument.

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

Unlike most backends, pinned top-level `npm:` versions do not bypass `install_before`: if the
selected package version was released on or after the cutoff, the install fails.

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="npm" :level="3" />
