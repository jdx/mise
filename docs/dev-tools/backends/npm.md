# npm Backend

You may install packages directly from [npmjs.org](https://npmjs.org/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).

## Dependencies

By default mise handles `npm:` tools without needing node or a package manager
CLI installed. Version resolution (`mise ls-remote`, resolving `latest`) queries
the npm registry directly over HTTP, and packages are installed with mise's
embedded [aube](https://github.com/jdx/aube) package manager. The registry,
scoped registries (`@scope:registry`), and auth tokens configured in `~/.npmrc`
(or `NPM_CONFIG_USERCONFIG`) and `NPM_CONFIG_*` environment variables are honored
by both. `node` is only needed to _run_ the installed tools (and any package
lifecycle scripts), not to install them.

To shell out to the npm CLI instead — `npm view` for metadata and
`npm install -g` for installs — set
[`npm.shell_out`](/configuration/settings.html#npm-shell-out) (requires npm to
be installed). Use it if you rely on npm-only configuration the built-in
implementation does not support, such as `cafile`, client certificates, or an
auth token helper.

You can also pick a specific installer with
[`npm.package_manager`](/configuration/settings.html#npm-package-manager). The
default `auto` uses the embedded aube; setting it to `bun`, `pnpm`, or `npm`
shells out to that tool, which must then be installed.

The npm backend forwards [`minimum_release_age`](/configuration/settings.html#minimum_release_age)
to transitive dependency resolution during install. The embedded aube installer
honors it natively. When shelling out, it relies on the package manager
supporting its release-age flag:

- `pnpm >= 10.16.0` using `--config.minimumReleaseAge=<minutes>`
- `bun >= 1.3.0` using `--minimum-release-age <seconds>`
- `npm >= 11.10.0` using `--min-release-age=<days>`; `npm 6.9.0–11.9.x` using `--before <timestamp>` (sub-day `minimum_release_age` windows also use `--before` since `--min-release-age` is day-granular)

If you want transitive protection when shelling out, install and use a package
manager version that meets the corresponding requirement above. Older versions
may fail while processing the forwarded argument.

`node` is installed automatically as a dependency. To use a specific package
manager CLI instead of the embedded installer:

```sh
mise use -g pnpm
# or
mise use -g bun
```

## Socket security

There are two ways to use [Socket](https://socket.dev) with `npm:` tools installed
by mise.

### Bun-compatible security scanner

The embedded aube installer implements
[Bun's Security Scanner API](https://bun.sh/docs/pm/security-scanner-api) and is
compatible with Socket's
[`@socketsecurity/bun-security-scanner`](https://socket.dev/blog/socket-integrates-with-bun-1-3-security-scanner-api).
Set `AUBE_SECURITY_SCANNER` to enable it:

```sh
MISE_NPM_PACKAGE_MANAGER=aube \
AUBE_SECURITY_SCANNER=/absolute/path/to/scanner.mjs \
  mise install npm:prettier@latest
```

Selecting `aube` explicitly ensures the scanner is used even if the user's
mise settings otherwise select npm, Bun, or pnpm.

The scanner runs after dependency resolution and before package tarballs are
downloaded. It receives the resolved direct and transitive registry packages;
a fatal finding blocks the install. A configured scanner also fails closed if
it cannot start or complete. See
[aube's security scanner documentation](https://aube.jdx.dev/package-manager/security-scanner.html)
for the complete behavior and configuration.

Mise installs each `npm:` tool in a synthetic project, so a bare scanner package
name is not normally resolvable from that project's `node_modules`. Point the
setting at an absolute module instead. For example, install the Socket scanner
in a separate, stable directory and place this wrapper beside that directory's
`node_modules`:

```js
// scanner.mjs
export { scanner } from "@socketsecurity/bun-security-scanner";
```

The scanner bridge requires Node.js 22.6 or newer. It inherits Socket-specific
environment variables such as `SOCKET_SECURITY_API_KEY`, while aube removes
common npm and GitHub credentials from the scanner subprocess.

### Socket Firewall

[Socket Firewall](https://docs.socket.dev/docs/socket-firewall-free) can instead
wrap mise itself:

```sh
sfw mise install npm:prettier@latest
sfw mise use -g npm:prettier
```

This works at the network layer. Mise's npm metadata client and embedded aube
installer honor the `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, and
`NODE_EXTRA_CA_CERTS` configuration injected by the firewall. Socket currently
documents npm, yarn, and pnpm rather than mise or aube as supported JavaScript
package managers, so this interoperability is not an upstream compatibility
guarantee.

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

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="npm" :level="3" />

## Lifecycle Scripts

The npm backend installs one global tool package at a time. Lifecycle scripts are
package-provided commands such as `preinstall`, `install`, `postinstall`, and `prepare`;
allowing them means allowing code from the selected package and its dependencies to run during
installation.

With the default `npm.package_manager = "auto"` setting, mise installs through its embedded `aube`
package manager. Setting `npm.package_manager = "aube"`, `"pnpm"`, `"bun"`, or `"npm"` chooses a
package manager explicitly (`aube` also uses the embedded one; the others shell out).
[`npm.shell_out`](/configuration/settings.html#npm-shell-out) forces the npm CLI. The `allow_builds`,
`trust_policy_excludes`, `pnpm_args`, `bun_args`, and `npm_args` options only affect the package
manager that is actually used; an approval option for one does not change the behavior of another.

For tools that need reviewed dependency build scripts, use `allow_builds` with `aube` (default),
`pnpm`, or npm 11.16.0+.

### `aube` (default)

The embedded [`aube`](https://aube.jdx.dev/package-manager/lifecycle-scripts) installer follows the
pnpm v11 build approval model: dependency lifecycle scripts are denied unless explicitly allowlisted.
Use `allow_builds` for reviewed dependency builds:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild"] }
```

`allow_builds` is written to the install's `aube.allowBuilds` manifest field.
Use `trust_policy_excludes` for reviewed aube trust-policy exceptions.
Set `allow_builds = true` to allow every dependency build script when you explicitly accept the risk.
(The `aube_args` option is ignored now that installs run in-process rather than via the `aube` CLI.)

### `pnpm`

[`pnpm`](https://pnpm.io/cli/add#--allow-build) uses build approval settings for dependency
lifecycle scripts. Use `allow_builds` for reviewed dependency builds:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild"] }
```

`allow_builds` is passed to `pnpm add --global` as one `--allow-build=<pkg>` flag per package.
`--allow-build` was added in pnpm v10.4.0 and is supported by pnpm v10.4.0+ and v11.x.
Set `allow_builds = true` to pass `--dangerously-allow-all-builds` when you explicitly accept that
every dependency build script may run.

[`pnpm approve-builds`](https://pnpm.io/cli/approve-builds) was added in v10.1.0, but it is not
recommended to run `approve-builds` from postinstall. `pnpm approve-builds -g` worked for global
packages in pnpm v10.4.0 through v10.x and was removed in v11.0.0; use
`allow_builds = ["<pkg>"]` for global installs with pnpm v10.4.0+ or v11.x.

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

`npm` normally runs lifecycle scripts by default. mise passes
[`--ignore-scripts=true`](https://docs.npmjs.com/cli/v11/using-npm/config/#ignore-scripts) by
default for npm-backed installs.

With npm 11.16.0+, `allow_builds = ["<pkg>"]` is passed as
[`--allow-scripts=<pkg>`](https://docs.npmjs.com/cli/v11/using-npm/config/#allow-scripts) for
reviewed global installs. When `allow_builds` is used and npm supports `--allow-scripts`, mise does
not pass `--ignore-scripts=true` because npm's `ignore-scripts` setting takes precedence over the
allowlist.

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild"] }
```

Set `allow_builds = true` to pass
[`--dangerously-allow-all-scripts`](https://docs.npmjs.com/cli/v11/using-npm/config/#dangerously-allow-all-scripts)
when you explicitly accept that every dependency build script may run.

For older npm versions, mise keeps `--ignore-scripts=true`; use `aube`/`pnpm`, upgrade npm, or opt
into npm's default script behavior with `npm_args` when you accept that every package in the install
graph can run lifecycle scripts:

```toml
[tools]
"npm:some-tool" = { version = "latest", npm_args = "--ignore-scripts=false" }
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `npm` backend. These
go in `[tools]` in `mise.toml`.

### `allow_builds`

Packages whose dependency lifecycle build scripts should be approved when
`settings.npm.package_manager = "aube"`, `"pnpm"`, or npm 11.16.0+. Use this instead of spelling out
package-manager-specific approval flags in `aube_args`, `pnpm_args`, or `npm_args`.

For example, to allow one verified dependency build script:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild"] }
```

For multiple reviewed dependency builds:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild", "sharp"] }
```

To allow all dependency build scripts for the install:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = true }
```

`allow_builds` does not affect `bun` installs because mise's Bun path is a global install and does
not write a per-transitive `trustedDependencies` allowlist. For npm installs, `allow_builds`
requires npm 11.16.0+.

### `trust_policy_excludes`

Packages or package version ranges that should be exempt from aube's `trustPolicy=no-downgrade`
check when `settings.npm.package_manager = "aube"`. Use this for reviewed dependency provenance
metadata churn without disabling the trust policy for the whole install.

For example, to exempt every version of a dependency:

```toml
[tools]
"npm:some-tool" = { version = "latest", trust_policy_excludes = ["undici"] }
```

To exempt only selected versions, use aube's package-version pattern syntax:

```toml
[tools]
"npm:some-tool" = { version = "latest", trust_policy_excludes = ["undici@^5 || >=6 <7"] }
```

`trust_policy_excludes` is written to the aube install `.npmrc` as `trustPolicyExclude`. It does
not affect `npm`, `pnpm`, or `bun` installs.

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

### `pnpm_args`

Additional arguments to pass to `pnpm` installs when `settings.npm.package_manager = "pnpm"`.
These are raw user-supplied arguments.

For example, to set pnpm's log level:

```toml
[tools]
"npm:some-tool" = { version = "latest", pnpm_args = "--loglevel=warn" }
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
