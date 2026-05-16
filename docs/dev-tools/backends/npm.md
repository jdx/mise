# npm Backend

You may install packages directly from [npmjs.org](https://npmjs.org/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).

## Dependencies

This relies on having `npm` installed for resolving package versions.
With the default `npm.package_manager = "auto"` setting, mise uses
[`aube`](https://aube.en.dev/) for installing npm packages when it is installed,
similar to how the pipx backend uses `uv` when available.
If you use `aube`, `bun`, or `pnpm` as the package manager, that package manager
must also be installed.

When [`minimum_release_age`](/configuration/settings.html#minimum_release_age) is set, the npm backend
forwards that cutoff to transitive dependency resolution during install. This relies on the
configured package manager supporting its native release-age flag:

- `npm >= 11.10.0` using `--min-release-age=<days>`; `npm 6.9.0–11.9.x` using `--before <timestamp>` (sub-day `minimum_release_age` windows also use `--before` since `--min-release-age` is day-granular)
- `aube` using its `minimumReleaseAge` setting
- `bun >= 1.3.0` using `--minimum-release-age <seconds>`
- `pnpm >= 10.16.0` using `--config.minimumReleaseAge=<minutes>`

If you want transitive protection, install and use a package manager version that meets the
corresponding requirement above. Older versions may fail while processing the forwarded argument.

Here is how to install `npm` with mise:

```sh
mise use -g node
```

To install `aube`, `bun`, or `pnpm`:

```sh
mise use -g aube
# or
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

## Lifecycle Scripts

The npm backend installs one global tool package at a time. Package managers differ in whether
dependency lifecycle scripts run automatically and how narrowly they can be approved:

| Package manager | Default dependency script behavior | Selective approval for mise installs |
| --- | --- | --- |
| `npm` | Runs lifecycle scripts by default. | No native per-dependency approval allowlist was found in npm's current docs. Disable scripts with `npm_args = "--ignore-scripts=true"` or `install_env = { NPM_CONFIG_IGNORE_SCRIPTS = "true" }`. |
| `bun` | Blocks arbitrary dependency lifecycle scripts unless the package is trusted. | Bun uses `trustedDependencies`, `bun add --trust`, and `bun pm trust` for project installs. The npm backend's Bun path is a global install and does not write a per-transitive `trustedDependencies` allowlist, so `bun_args` should not be used in registry entries to approve required dependency builds unless Bun adds a narrow global approval flag. Users may still pass raw `bun_args` manually. |
| `pnpm` | Blocks unreviewed dependency build scripts under its build-approval settings. | Use `pnpm_args = "--allow-build=<pkg>"` once per dependency that must run `preinstall`, `install`, or `postinstall`. `pnpm approve-builds -g` exists in pnpm 10.x, but pnpm 11 removes that global form and documents `--allow-build` for global installs. pnpm 10 writes approvals to `onlyBuiltDependencies` until newer 10.x/11.x `allowBuilds` support is available. |
| `aube` | Blocks dependency lifecycle scripts unless approved through `allowBuilds`; `strictDepBuilds` can make unreviewed builds fail the install. | Use `aube_args = "--allow-build=<pkg>"` with aube 1.12.0 or newer. Older aube releases did not forward `--allow-build` into global installs. `aube approve-builds -g` can update existing global installs, but it is a follow-up command, not something mise runs during installation. |

Avoid broad registry flags such as `--trust`, `--ignore-scripts=false`, or
`--dangerously-allow-all-builds`. Registry entries should only approve the exact dependency packages
whose lifecycle scripts were verified as required.

Related package-manager docs:

- npm [`ignore-scripts`](https://docs.npmjs.com/cli/v8/using-npm/config/#ignore-scripts)
- Bun [lifecycle scripts](https://bun.sh/docs/pm/cli/install#lifecycle-scripts) and [`bun pm trust`](https://bun.com/docs/pm/cli/pm#trust)
- pnpm [`--allow-build`](https://pnpm.io/cli/add#--allow-build), [`approve-builds`](https://pnpm.io/cli/approve-builds), and [build settings](https://pnpm.io/settings#allowbuilds)
- aube [lifecycle scripts](https://aube.en.dev/package-manager/lifecycle-scripts), [security](https://aube.en.dev/security.html#default-deny-lifecycle-scripts), and [settings](https://aube.en.dev/settings/#allowbuilds)

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `npm` backend—these
go in `[tools]` in `mise.toml`.

### `npm_args`

Additional arguments to pass to `npm` installs when `settings.npm.package_manager = "npm"`.

For example, to disable lifecycle scripts for one npm-backed tool:

```toml
[tools]
"npm:prettier" = { version = "latest", npm_args = "--ignore-scripts=true" }
```

### `pnpm_args`

Additional arguments to pass to `pnpm` installs when `settings.npm.package_manager = "pnpm"`.

For example, to allow one verified dependency build script:

```toml
[tools]
"npm:some-tool" = { version = "latest", pnpm_args = "--allow-build=esbuild" }
```

### `bun_args`

Additional arguments to pass to `bun` installs when `settings.npm.package_manager = "bun"`.
These are raw user-supplied arguments. mise does not add `--trust` automatically.

### `aube_args`

Additional arguments to pass to `aube add --global` when
`settings.npm.package_manager = "aube"`.

For example, to install `npm` with aube's append-only reporter mode:

```toml
[tools]
"npm:npm" = {
  version = "latest",
  aube_args = "--reporter append-only",
}
```

For build approvals, use aube 1.12.0 or newer:

```toml
[tools]
"npm:some-tool" = {
  version = "latest",
  aube_args = "--allow-build=esbuild",
  install_env = { AUBE_STRICT_DEP_BUILDS = "true", AUBE_JAIL_BUILDS = "true" },
}
```
