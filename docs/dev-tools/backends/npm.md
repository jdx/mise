# npm Backend

You may install packages directly from [npmjs.org](https://npmjs.org/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).

## Dependencies

This relies on having `npm` installed for resolving package versions.
With the default `npm.package_manager = "auto"` setting, mise uses
[`aube`](https://aube.en.dev/) for installing npm packages when it is installed,
similar to how the pipx backend uses `uv` when available.
If you use `aube`, `pnpm`, or `bun` as the package manager, that package manager
must also be installed.

When [`minimum_release_age`](/configuration/settings.html#minimum_release_age) is set, the npm backend
forwards that cutoff to transitive dependency resolution during install. This relies on the
configured package manager supporting its native release-age flag:

- `aube` using its `minimumReleaseAge` setting
- `pnpm >= 10.16.0` using `--config.minimumReleaseAge=<minutes>`
- `bun >= 1.3.0` using `--minimum-release-age <seconds>`
- `npm >= 11.10.0` using `--min-release-age=<days>`; `npm 6.9.0–11.9.x` using `--before <timestamp>` (sub-day `minimum_release_age` windows also use `--before` since `--min-release-age` is day-granular)

If you want transitive protection, install and use a package manager version that meets the
corresponding requirement above. Older versions may fail while processing the forwarded argument.

Here is how to install `npm` with mise:

```sh
mise use -g node
```

To install `aube`, `pnpm`, or `bun`:

```sh
mise use -g aube
# or
mise use -g pnpm
# or
mise use -g bun
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

## Lifecycle Scripts

The npm backend installs one global tool package at a time. Lifecycle scripts are
package-provided commands such as `preinstall`, `install`, `postinstall`, and `prepare`;
allowing them means allowing code from the selected package and its dependencies to run during
installation.

With the default `npm.package_manager = "auto"` setting, mise installs through `aube` when `aube` is
installed. If `aube` is not installed, mise installs through `npm`. Setting
`npm.package_manager = "aube"`, `"pnpm"`, `"bun"`, or `"npm"` chooses that package manager
explicitly. The `aube_args`, `pnpm_args`, `bun_args`, and `npm_args` options only affect the package
manager that is actually used; an approval option for one package manager does not change the
behavior of another.

For tools that need reviewed dependency build scripts, consider using `aube` or `pnpm` because they
support package-level build approvals.

### `aube`

[`aube`](https://aube.en.dev/package-manager/lifecycle-scripts) follows the pnpm v11 build approval
model: dependency lifecycle scripts are denied unless explicitly allowlisted. Use `aube_args` to pass
CLI flags to `aube add --global`:

```toml
[tools]
"npm:some-tool" = { version = "latest", aube_args = "--allow-build=esbuild" }
```

Repeat `--allow-build=<pkg>` for each reviewed dependency build.

### `pnpm`

[`pnpm`](https://pnpm.io/cli/add#--allow-build) uses build approval settings for dependency
lifecycle scripts. Use `pnpm_args` to pass CLI flags to `pnpm add --global`:

```toml
[tools]
"npm:some-tool" = { version = "latest", pnpm_args = "--allow-build=esbuild" }
```

Repeat `--allow-build=<pkg>` for each reviewed dependency build. `--allow-build` was added in pnpm
v10.4.0 and is supported by pnpm v10.4.0+ and v11.x.

[`pnpm approve-builds`](https://pnpm.io/cli/approve-builds) was added in v10.1.0, but it is not
recommended to run `approve-builds` from postinstall. `pnpm approve-builds -g` worked for global
packages in pnpm v10.4.0 through v10.x and was removed in v11.0.0; use
`pnpm_args = "--allow-build=<pkg>"` for global installs with pnpm v10.4.0+ or v11.x.

### `bun`

[`bun`](https://bun.sh/docs/pm/lifecycle) does not execute arbitrary dependency lifecycle scripts by
default. Bun's project install controls include `trustedDependencies`, `bun add --trust`, and
`bun pm trust`, but the npm backend's Bun path is a global install and does not write a
per-transitive `trustedDependencies` allowlist.

mise does not add Bun's [`--trust`](https://bun.sh/docs/pm/cli/add#trusted-dependencies) flag
automatically. You can pass it explicitly with `bun_args` when you accept that broader install-time
script trust:

```toml
[tools]
"npm:some-tool" = { version = "latest", bun_args = "--trust" }
```

### `npm`

`npm` normally runs lifecycle scripts by default and does not provide a native per-dependency build
approval allowlist in its current docs. mise passes
[`--ignore-scripts=true`](https://docs.npmjs.com/cli/v11/using-npm/config/#ignore-scripts) by
default for npm-backed installs.

You can opt into npm's default script behavior with `npm_args` when you accept that every package in
the install graph can run lifecycle scripts:

```toml
[tools]
"npm:some-tool" = { version = "latest", npm_args = "--ignore-scripts=false" }
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `npm` backend. These
go in `[tools]` in `mise.toml`.

### `aube_args`

Additional arguments to pass to `aube add --global` when
`settings.npm.package_manager = "aube"`.
These are raw user-supplied arguments.

For example, to allow one verified dependency build script:

```toml
[tools]
"npm:some-tool" = { version = "latest", aube_args = "--allow-build=esbuild" }
```

Or to install `npm` with aube's append-only reporter mode:

```toml
[tools]
"npm:npm" = {
  version = "latest",
  aube_args = "--reporter append-only",
}
```

### `pnpm_args`

Additional arguments to pass to `pnpm` installs when `settings.npm.package_manager = "pnpm"`.
These are raw user-supplied arguments.

For example, to allow one verified dependency build script:

```toml
[tools]
"npm:some-tool" = { version = "latest", pnpm_args = "--allow-build=esbuild" }
```

### `bun_args`

Additional arguments to pass to `bun` installs when `settings.npm.package_manager = "bun"`.
These are raw user-supplied arguments. mise does not add `--trust` automatically.

For example, to pass Bun's broad trust flag:

```toml
[tools]
"npm:some-tool" = { version = "latest", bun_args = "--trust" }
```

### `npm_args`

Additional arguments to pass to `npm` installs when `settings.npm.package_manager = "npm"`.
These are raw user-supplied arguments. For example, to opt into npm lifecycle scripts:

```toml
[tools]
"npm:some-tool" = { version = "latest", npm_args = "--ignore-scripts=false" }
```
