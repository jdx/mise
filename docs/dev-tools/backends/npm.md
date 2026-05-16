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

The npm backend installs one global tool package at a time. Lifecycle scripts are package-provided
commands such as `preinstall`, `install`, and `postinstall`; allowing them means allowing code from
the selected package and its dependencies to run during installation.

With the default `npm.package_manager = "auto"` setting, mise installs through `aube` when `aube` is
installed. If `aube` is not installed, mise installs through `npm`. Setting
`npm.package_manager = "npm"`, `"bun"`, `"pnpm"`, or `"aube"` chooses that package manager
explicitly. The `npm_args`, `bun_args`, `pnpm_args`, and `aube_args` options only affect the package
manager that is actually used; an approval option for one package manager does not change the
behavior of another.

### `npm`

`npm` runs lifecycle scripts by default. Available controls:

- Disable lifecycle scripts with `npm_args = "--ignore-scripts=true"` or
  `install_env = { NPM_CONFIG_IGNORE_SCRIPTS = "true" }`.
- Explicitly setting `npm_args = "--ignore-scripts=false"` keeps npm's default script behavior, or
  forces scripts back on when another npm config source disabled them. In a registry entry, this
  would override a user's script-disabling config and allow every package in the install graph to run
  scripts.

`npm` does not provide a native per-dependency build approval allowlist in its current docs.

### `bun`

`bun` does not execute arbitrary dependency lifecycle scripts by default. Available controls:

- `trustedDependencies` in `package.json` and `bun pm trust <pkg>` approve specific dependencies for
  project installs.
- `bun_args = "--trust"` passes Bun's trust flag. For project installs, Bun documents this as adding
  the package to `trustedDependencies` and installing it. This is broad for npm-backend global
  installs and is a user escape hatch, not something registry entries should use.
- `bun_args = "--ignore-scripts"` disables lifecycle scripts for the install.
- Bun also reads install script settings from `.npmrc` and `bunfig.toml`, such as `ignore-scripts` /
  `install.ignoreScripts`.

The npm backend's Bun path is a global install. It does not write a per-transitive
`trustedDependencies` allowlist, and Bun does not currently document a narrow global CLI flag for
approving one transitive dependency build in this flow.

### `pnpm`

`pnpm` uses build approval settings for dependency lifecycle scripts. Available controls:

- `pnpm_args = "--allow-build=<pkg>"` pre-approves one dependency package for the global install.
  Repeat the flag once per package that has a verified required build.
- `pnpm approve-builds` is the interactive project approval command.
- `strictDepBuilds` can make unreviewed builds fail instead of warning.
- `dangerouslyAllowAllBuilds = true` allows all dependency build scripts. This is broad and should
  not be used in registry entries.

pnpm 10 and 11 differ here:

- pnpm 10 supports `pnpm approve-builds -g` for globally installed packages. Older pnpm 10 approval
  flows use `onlyBuiltDependencies` and `ignoredBuiltDependencies`; newer pnpm 10 releases also
  support the `allowBuilds` map, which replaces those fields.
- pnpm 11 removes `pnpm approve-builds -g`. For global installs, use
  `pnpm_args = "--allow-build=<pkg>"` during install; pnpm 11 records reviewed project approvals in
  the `allowBuilds` map.

### `aube`

`aube` follows the pnpm v11 build approval model for dependency lifecycle scripts. Available
controls:

- `aube_args = "--allow-build=<pkg>"` pre-approves one dependency package for the global install.
  Repeat the flag once per package that has a verified required build.
- `aube approve-builds -g` can update approvals for globally installed packages after installation.
- `allowBuilds` records allowed and denied dependency builds.
- `strictDepBuilds` / `AUBE_STRICT_DEP_BUILDS` can make unreviewed builds fail.
- `jailBuilds` / `AUBE_JAIL_BUILDS` can restrict approved dependency builds.
- `aube_args = "--ignore-scripts"` skips root lifecycle scripts; dependency scripts are already
  denied by default unless approved.

### Registry Policy

Registry entries should only approve the exact dependency packages whose lifecycle scripts were
verified as required. Avoid broad registry options such as `--trust` or
`dangerouslyAllowAllBuilds = true`; those settings expand the install-time code execution surface to
packages that were not individually reviewed. Also avoid `--ignore-scripts=false` in registry entries
because it can override a user's explicit script-disabling config. Users may still pass broad raw
args in their own tool options when they accept that supply chain risk.

Related package-manager docs:

- npm [`ignore-scripts`](https://docs.npmjs.com/cli/v11/using-npm/config/#ignore-scripts)
- Bun [lifecycle scripts](https://bun.sh/docs/install/lifecycle), [`bun add`](https://bun.sh/docs/pm/cli/add), and [`bun pm trust`](https://bun.sh/docs/cli/pm#trust)
- pnpm 11 [`--allow-build`](https://pnpm.io/cli/add#--allow-build), [`approve-builds`](https://pnpm.io/cli/approve-builds), and [build settings](https://pnpm.io/settings#allowbuilds)
- pnpm 10 [`approve-builds`](https://pnpm.io/10.x/cli/approve-builds) and [build settings](https://pnpm.io/10.x/settings#onlybuiltdependencies)
- aube [lifecycle scripts](https://aube.en.dev/package-manager/lifecycle-scripts), [security](https://aube.en.dev/security.html#default-deny-lifecycle-scripts), and [settings](https://aube.en.dev/settings/#allowbuilds)

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `npm` backend. These
go in `[tools]` in `mise.toml`.

### `install_env`

Set environment variables for the configured package manager install command.
mise still controls install destination variables such as `BUN_INSTALL_GLOBAL_DIR`
and `BUN_INSTALL_BIN` after applying `install_env`.

```toml
[tools]
"npm:prettier" = { version = "latest", install_env = { NPM_CONFIG_REGISTRY = "https://registry.npmjs.org/" } }
```

### `npm_args`

Additional arguments to pass to `npm` installs when `settings.npm.package_manager = "npm"`.
These are raw user-supplied arguments.

For example, to disable lifecycle scripts for one npm-backed tool:

```toml
[tools]
"npm:prettier" = { version = "latest", npm_args = "--ignore-scripts=true" }
```

You can also pass npm's default explicitly, or force scripts back on when another npm config source
disabled them. This broadly allows scripts for the full install graph:

```toml
[tools]
"npm:some-tool" = { version = "latest", npm_args = "--ignore-scripts=false" }
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

For example, to pass Bun's broad trust flag manually:

```toml
[tools]
"npm:some-tool" = { version = "latest", bun_args = "--trust" }
```

Or to skip scripts:

```toml
[tools]
"npm:some-tool" = { version = "latest", bun_args = "--ignore-scripts" }
```

### `aube_args`

Additional arguments to pass to `aube add --global` when
`settings.npm.package_manager = "aube"`.
These are raw user-supplied arguments.

For example, to install `npm` with aube's append-only reporter mode:

```toml
[tools]
"npm:npm" = {
  version = "latest",
  aube_args = "--reporter append-only",
}
```

For build approvals:

```toml
[tools]
"npm:some-tool" = {
  version = "latest",
  aube_args = "--allow-build=esbuild",
  install_env = { AUBE_STRICT_DEP_BUILDS = "true", AUBE_JAIL_BUILDS = "true" },
}
```
