---
description: "Install npm command-line packages into separate tool directories."
---

# npm Backend

The `npm` backend installs command-line packages from npm registries into
separate tool directories. Keep your application's dependencies in `package.json`
and use its package manager or [mise deps](/dev-tools/deps.html) to install them.

## Quick start {#usage}

Prettier needs Node.js at runtime, so declare both tools in the current project:

```sh
mise use node@24 npm:prettier
mise exec -- prettier --version
```

This writes the following to `mise.toml`. Add `-g` for global configuration.

```toml
[tools]
node = "24"
"npm:prettier" = "latest"
```

For a scoped package, quote its full identifier, for example
`mise use 'npm:@biomejs/biome'`. The package's executable name can differ from its
registry name. mise installs CLI packages, not arbitrary libraries.

If the project already declares Prettier in `package.json`, run that copy through
a package script to keep its plugins and version aligned with the project.

## Dependencies

mise installs npm tools with its embedded [aube](https://github.com/jdx/aube)
package manager by default. Version lookup and installation do not require a
separate Node.js or package-manager executable.

**The installed CLI may still need Node.js**, as may its lifecycle scripts.
Declare `node` in `[tools]` when needed. mise installs a configured Node.js
before npm tools, but does not add it to your project automatically.

## Choosing an installer

Use [`npm.package_manager`](/configuration/settings.html#npm.package_manager)
to select the installer:

| Setting          | Installer                                         | Separate executable required |
| ---------------- | ------------------------------------------------- | ---------------------------- |
| `auto` (default) | Embedded aube, or npm when `npm.shell_out = true` | Only when using npm          |
| `aube`           | Embedded aube                                     | No                           |
| `aube_cli`       | `aube add --global`                               | `aube`                       |
| `pnpm`           | pnpm                                              | `pnpm`                       |
| `bun`            | Bun                                               | `bun`                        |
| `npm`            | npm                                               | `npm`                        |

For example:

```toml
[settings.npm]
package_manager = "pnpm"
```

An explicit installer takes precedence over `npm.shell_out` for installation.
Installer-specific options apply only to the selected installer.
Standalone `aube_cli` invokes aube directly; it does not need `aube activate`
or an npm compatibility shim.

### Registry configuration

By default, mise queries the registry directly over HTTP for version lookup.
Both that client and embedded aube honor registries, scoped registries
(`@scope:registry`), and auth tokens from `~/.npmrc`,
`NPM_CONFIG_USERCONFIG`, and `NPM_CONFIG_*` environment variables.

Set [`npm.shell_out`](/configuration/settings.html#npm.shell_out) to use
`npm view` for metadata and, with the default `auto` installer, `npm install -g`
for installation. This requires npm. Use it for npm-specific configuration the
built-in client does not support, such as `cafile`, client certificates, or an
auth token helper.

## Dependency locking

With the embedded aube installer, version-2 lockfiles record the tool's
transitive dependency graph. Create a lockfile or upgrade an existing one:

```sh
mise lock --upgrade
mise install --locked
```

To refresh dependencies even when the tool's own version has not changed:

```sh
mise lock --bump npm:prettier
```

Ordinary locking reuses the recorded graph. Frozen installs replay it without
resolving dependencies again. Other installers cannot replay an embedded-aube
graph; use embedded aube or refresh the lockfile for the selected installer.

### Dependency sidecars

Commit the native `package.json` and `aube-lock.yaml` files alongside `mise.lock`.
They live in [per-entry sidecar directories](../mise-lock.md#native-dependency-sidecars),
usually `.mise/locks/npm-<package>/<version>/`.

No-op locking preserves the native file bytes. The format is aube's YAML,
not `package-lock.json`, so npm-only scanners may not recognize its transitive
dependencies. See the [lockfile guide](../mise-lock.md#dependency-graphs) for
editing sidecars and validating their digests.

## Minimum release age

mise forwards [`minimum_release_age`](/configuration/settings.html#minimum_release_age)
to transitive dependency resolution. Embedded aube handles it natively. For a
frozen graph, the cutoff applies when resolving the graph; installation replays
the committed dependencies.

External installers need a version that supports the forwarded flag:

| Installer | Minimum version | Flag                                   |
| --------- | --------------- | -------------------------------------- |
| pnpm      | 10.16.0         | `--config.minimumReleaseAge=<minutes>` |
| Bun       | 1.3.0           | `--minimum-release-age <seconds>`      |
| npm       | 6.9.0           | `--before <timestamp>`                 |
| npm       | 11.10.0         | `--min-release-age=<days>`             |

npm still uses `--before` for sub-day windows because `--min-release-age` only
accepts whole days. Older package-manager versions may fail on the forwarded
argument.

## Lifecycle Scripts

The npm backend installs one global tool package at a time. Lifecycle scripts are
package-provided commands such as `preinstall`, `install`, `postinstall`, and `prepare`;
allowing them means allowing code from the selected package and its dependencies to run during
installation.

For reviewed dependency builds, use `allow_builds` with embedded aube,
standalone aube, pnpm, or npm 11.16.0+:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild"] }
```

The policy depends on the [selected installer](#choosing-an-installer).
Approvals for one installer do not change another's behavior.

### `aube` (default)

Embedded [aube](https://aube.jdx.dev/package-manager/lifecycle-scripts)
denies dependency lifecycle scripts unless explicitly allowlisted, following
the pnpm v11 build approval model. mise writes `allow_builds` to the install's
`aube.allowBuilds` manifest field. `allow_builds = true` allows every dependency
build script.

Use `trust_policy_excludes` for reviewed trust-policy exceptions.
`aube_args` is ignored by the embedded installer.

### `aube_cli`

Standalone aube receives `allow_builds` and `aube_args` through
`aube add --global`. Select it with `npm.package_manager = "aube_cli"`.

### `pnpm`

With pnpm 10.4.0+ and v11, mise passes each `allow_builds` package as
[`--allow-build=<pkg>`](https://pnpm.io/cli/add#--allow-build).
`allow_builds = true` passes `--dangerously-allow-all-builds`.

Use this option for global installs rather than running `pnpm approve-builds`
from postinstall. Global `approve-builds -g` was available in pnpm 10.4.0–10.x
and removed in v11.

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

`npm` runs lifecycle scripts by default. mise passes
[`--ignore-scripts=true`](https://docs.npmjs.com/cli/v11/using-npm/config/#ignore-scripts) by
default for npm-backed installs.

With npm 11.16.0+, `allow_builds = ["<pkg>"]` is passed as
[`--allow-scripts=<pkg>`](https://docs.npmjs.com/cli/v11/using-npm/config/#allow-scripts) for
reviewed global installs. When `allow_builds` is used and npm supports `--allow-scripts`, mise does
not pass `--ignore-scripts=true` because npm's `ignore-scripts` setting takes precedence over the
allowlist.

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
`settings.npm.package_manager = "aube"`, `"aube_cli"`, `"pnpm"`, or npm 11.16.0+. Use this instead
of spelling out package-manager-specific approval flags in `aube_args`, `pnpm_args`, or `npm_args`.

For example:

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
check when `settings.npm.package_manager = "aube"` or `"aube_cli"`. Use this for reviewed dependency
provenance metadata churn without disabling the trust policy for the whole install.

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

`trust_policy_excludes` is written to the aube install's `.config/aube/config.toml` as
`trustPolicyExclude`. It does not affect `npm`, `pnpm`, or `bun` installs.

### `allow_low_downloads`

Explicitly approves the requested package for aube's reputation checks. This includes a weekly
download count below `lowDownloadThreshold` (1000 by default), a name similar to a popular package,
or a newly registered package name. Without it, aube refuses; for example:

```
refusing to add some-tool: only 930 weekly downloads (threshold: 1000).
```

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_low_downloads = true }
```

The exemption is scoped to the package you asked for, written to the aube install's
`.config/aube/config.toml` under `allowedUnpopularPackages`. Transitive dependencies stay gated,
and aube's malicious-package advisory check still runs. The reputation thresholds themselves are
left alone, so this cannot silently admit an unapproved dependency.

An npm tool resolved from `mise.lock` is trusted automatically for these three reputation checks,
so reproducing an existing lockfile does not require `allow_low_downloads`. The explicit option is
still required to approve the first unlocked install.

These are reputation signals, not proof that a package is unsafe. Verify the package name and
publisher before approving it. This option does not affect `npm`, `pnpm`, or `bun` installs.

### `aube_args`

Additional arguments to pass to `aube add --global` when
`settings.npm.package_manager = "aube_cli"`.
These are raw user-supplied arguments.

For example, to install `npm` with aube's append-only reporter mode:

```toml
[tools]
"npm:npm" = { version = "latest", aube_args = "--reporter append-only" }
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

Additional arguments for npm installs, selected with `npm.package_manager = "npm"`
or with `auto` and `npm.shell_out = true`.
These are raw user-supplied arguments. For example, to opt into npm lifecycle scripts:

```toml
[tools]
"npm:some-tool" = { version = "latest", npm_args = "--ignore-scripts=false" }
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

mise installs each `npm:` tool in a synthetic project, so a bare scanner package
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

This works at the network layer. mise's npm metadata client and embedded aube
installer both use aube-registry, which honors the `HTTP_PROXY`, `HTTPS_PROXY`,
and `NO_PROXY` settings and explicitly loads the `NODE_EXTRA_CA_CERTS` bundle
into its Rust TLS clients. Socket currently documents npm, yarn, and pnpm rather
than mise or aube as supported JavaScript package managers, so this
interoperability is not an upstream compatibility guarantee.

## Troubleshooting

- **`node` is missing when the CLI starts:** configure Node.js explicitly; the embedded installer does not add a runtime to your project.
- **A native dependency is missing:** inspect the selected installer's lifecycle-script policy and approve only the required builds using its supported option.
- **Private package metadata works but installation fails:** check the installer you selected and whether both clients can read the registry and credentials.
- **Aube trust or download-count policy blocks installation:** inspect the specific policy error and the relevant option above before changing installers.

### Investigating trust downgrades

A `trustPolicy=no-downgrade` failure is a supply-chain signal, not an ordinary inability to find a
matching version. It means an earlier release had stronger npm trusted-publisher, staged-publish,
or provenance evidence than the selected release.

Before adding an exception:

1. Inspect the npm release, source tag/commit, publisher identity, and tarball, compare the metadata
   with npmjs.org, and confirm nothing appears tampered with.
2. Check whether the maintainer intentionally published manually, backported outside the trusted
   workflow, skipped provenance, or used a registry that stripped metadata.
3. Report inconsistent evidence to the relevant upstream owner. Package-release drift belongs with
   the maintainer; metadata present on npmjs.org but missing from a proxy or mirror belongs with
   that registry operator.
4. Prefer a version-scoped `"<package>@<version>"` exception after review. A bare package name
   exempts every future version.

With the default `auto` package manager, using `mise settings npm.shell_out=true` switches to the npm
CLI and bypasses this aube check entirely, so it should be a last resort rather than the first
workaround. An explicit `npm.package_manager = "aube_cli"` selection still uses standalone aube for
installation.

See aube's [trust-policy documentation](https://aube.jdx.dev/security#trust-policy) for more
detail.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="npm" :level="3" />

Implementation: [`src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).
