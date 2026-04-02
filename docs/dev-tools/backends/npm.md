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

Minimum supported package manager versions for the transitive dependency part are:

- `npm >= 6.9.0` using `--before` (`Node >= 10.16.0` if you rely on bundled npm)
- `bun >= 1.3.0` using `--minimum-release-age`
- `pnpm >= 10.16.0` using `--config.minimumReleaseAge=...`

If the configured package manager is older than that, npm backend installs fail with an upgrade hint
instead of silently skipping the supply-chain guard.

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
