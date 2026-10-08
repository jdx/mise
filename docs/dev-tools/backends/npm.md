---
description: "Install npm command-line packages, each in its own directory, with the embedded aube installer."
---

# npm backend

The `npm` backend installs command-line packages from npm registries, each into
its own directory. It installs them with mise's embedded
[aube](https://github.com/aubepkg/aube) package manager, so you need Node.js only
to run the tools. Keep your application's dependencies in `package.json` and
install them with its package manager or [mise deps](/dev-tools/deps.html).

## Requirements

<span id="dependencies"></span>

mise lists and installs npm tools with the embedded aube package manager, so it
needs neither Node.js nor npm for that. Most CLIs still need Node.js to run, and so do some
lifecycle scripts. Add `node` to `[tools]`: mise installs it before your npm
tools, but never adds it for you.

Other installers need their own executable; see
[choosing an installer](#choosing-an-installer).

## Usage {#usage}

<span id="quick-start"></span>

Prettier needs Node.js to run, so declare both tools in the current project:

```sh
mise use node@24 npm:prettier
mise exec -- prettier --version
```

This writes the following to `mise.toml`. Add `-g` for your global config.

```toml
[tools]
node = "24"
"npm:prettier" = "latest"
```

If your project already lists Prettier in `package.json`, run that copy through
a package script instead, so its version and plugins match the project.

For a scoped package, quote the identifier: `mise use 'npm:@biomejs/biome'`.
The command it installs is `biome`, not the package name.

### Install from Git

To install from a Git repository instead of the registry, use a Git URL or a
`github:`, `gitlab:` or `bitbucket:` shorthand. The version is a Git ref (a tag,
branch or commit), and `latest` is the repository's default branch:

```sh
mise use 'npm:git+https://github.com/owner/repo'
mise use 'npm:github:owner/repo@v1.2.0'
```

## Private registries {#registry-configuration}

mise reads package metadata from the registry over HTTP, and embedded aube
installs from it. Both read registries, scoped registries (`@scope:registry`),
auth tokens, `ca` and `cafile`, `strict-ssl`, per-registry `cert` and `key`
client certificates, and `tokenHelper` from your user `~/.npmrc` (or the file in
`NPM_CONFIG_USERCONFIG`) and from `NPM_CONFIG_*` environment variables. Neither
reads a `.npmrc` in your project.

Set [`npm.shell_out`](/dev-tools/backends/npm.html#npm.shell_out) to use `npm view` for metadata and, with
the default `auto` installer, `npm install -g` for installs. This requires npm.
Use it for npm-only behavior the built-in client does not replicate, or to debug
a registry problem with npm itself.

## Dependency locking

With the embedded aube installer, `mise.lock` records each npm tool's full
dependency graph (lockfile format 2 or later, which new lockfiles use). Create a
lockfile, or upgrade one written by an older mise, then install from it:

```sh
mise lock --upgrade
mise install --locked
```

For a new lockfile, plain `mise lock` is enough. To refresh a tool's
dependencies when its own version has not changed:

```sh
mise lock --bump npm:prettier
```

Ordinary locking reuses the recorded graph, and locked installs replay it
without resolving dependencies again. Other installers cannot replay an
embedded-aube graph; use embedded aube, or refresh the lockfile for the
installer you select.

<span id="dependency-sidecars"></span>

Commit the tool's sidecar directory (by default
`.mise/locks/npm-<package>/<version>/`) with `mise.lock`; `mise lock --sidecars`
lists it. It holds the graph as `package.json` and `aube-lock.yaml`, in aube's format rather
than `package-lock.json`, so scanners that read only npm lockfiles may miss its
dependencies. See [dependency graphs](/dev-tools/mise-lock.html#dependency-graphs).

## Minimum release age

mise applies [`minimum_release_age`](/configuration/settings.html#minimum_release_age)
to the tool's dependencies as well as the tool. Embedded aube handles it
itself. For a locked graph, the cutoff applies when the graph is resolved;
installs replay the committed dependencies.

External installers need a version that supports the flag mise passes:

| Installer | Minimum version | Flag                                                                                          |
| --------- | --------------- | --------------------------------------------------------------------------------------------- |
| pnpm      | 10.16.0         | `--config.minimum-release-age=<minutes>`                                                      |
| Bun       | 1.3.0           | `--minimum-release-age <seconds>`                                                             |
| npm       | 6.9.0           | `--before <timestamp>`; from 11.10.0, `--min-release-age=<days>` for windows of a day or more |

Older versions may reject the flag.

## Build scripts and supply-chain checks

<span id="lifecycle-scripts"></span>
<span id="allow-builds"></span>

Lifecycle scripts (`preinstall`, `install`, `postinstall`, `prepare`) run code
from the package and its dependencies during installation. By default mise does
not run dependency scripts. Approve specific packages with `allow_builds`:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_builds = ["esbuild", "sharp"] }
```

`allow_builds = true` allows every dependency script. How the option reaches
each [installer](#choosing-an-installer):

| Installer               | Without `allow_builds`                    | `allow_builds = ["pkg"]`                    | `allow_builds = true`             |
| ----------------------- | ----------------------------------------- | ------------------------------------------- | --------------------------------- |
| embedded aube (default) | Dependency scripts do not run             | Written to the install's `aube.allowBuilds` | Every dependency script runs      |
| `aube_cli`              | mise passes `--ignore-scripts`            | `--allow-build=<pkg>`                       | `--dangerously-allow-all-builds`  |
| `pnpm` 10.4 or later    | pnpm's default; pnpm 10 skips them        | `--allow-build=<pkg>`                       | `--dangerously-allow-all-builds`  |
| `npm` 11.16 or later    | mise passes `--ignore-scripts=true`       | `--allow-scripts=<pkg>`                     | `--dangerously-allow-all-scripts` |
| older `npm`             | mise passes `--ignore-scripts=true`       | Not supported; mise warns                   | Not supported; mise warns         |
| `bun`                   | Bun skips untrusted dependencies' scripts | Ignored                                     | Ignored                           |

Approvals for one installer do not change another's behavior. With npm 11.16 or
later, mise drops `--ignore-scripts=true` when `allow_builds` is set, because
npm's `ignore-scripts` setting would override the allowlist. With older npm,
upgrade npm, switch to aube or pnpm, or accept every script in the install with
`npm_args = "--ignore-scripts=false"`. mise installs with Bun globally and does
not write Bun's `trustedDependencies` list; to trust every script, pass
`bun_args = "--trust"`.

With pnpm, use `allow_builds` rather than running `pnpm approve-builds` from a
`postinstall` hook: global `approve-builds -g` existed only in pnpm 10.4 to 10.x
and was removed in pnpm 11.

### Trust policy

<span id="trust-policy-excludes"></span>

aube's `trustPolicy=no-downgrade` check fails an install when an earlier release
of a dependency had stronger npm trusted-publisher, staged-publish or provenance
evidence than the selected release. After reviewing such a dependency, exempt it
with `trust_policy_excludes`. It applies to embedded aube (the default) and
`aube_cli`:

```toml
[tools]
"npm:some-tool" = { version = "latest", trust_policy_excludes = ["undici@^5 || >=6 <7"] }
```

Use aube's package-version patterns to exempt only reviewed versions. A bare
package name, such as `"undici"`, exempts every future version too. mise writes
the list to the install's `.config/aube/config.toml` as `trustPolicyExclude`.
See [investigating trust downgrades](#investigating-trust-downgrades) before you
add an exception, and aube's
[trust-policy documentation](https://aube.sh/security#trust-policy).

### Low-download and new packages

<span id="allow-low-downloads"></span>

aube refuses to add a package that has fewer weekly downloads than its
`lowDownloadThreshold` (1000 by default), a name similar to a popular package, or
a newly registered name:

```text
refusing to add some-tool: only 930 weekly downloads (threshold: 1000).
```

After checking the package name and publisher, approve it with
`allow_low_downloads`:

```toml
[tools]
"npm:some-tool" = { version = "latest", allow_low_downloads = true }
```

The exemption covers only the package you asked for. mise writes it to the
install's `.config/aube/config.toml` under `allowedUnpopularPackages`; the
thresholds stay unchanged, dependencies are still checked, and aube's
malicious-package advisory check still runs. A tool resolved from `mise.lock`
passes these three checks automatically, so the option is needed only for the
first unlocked install. These are reputation signals, not proof that a package
is unsafe. The option applies to embedded aube and `aube_cli`.

### Dependencies from outside the registry

<span id="allow-exotic-deps"></span>

aube refuses dependencies that come from outside the npm registry (a `git+` URL,
a `file:` path or a tarball URL), because the registry's protections never see
them. Its [`blockExoticSubdeps`](https://aube.sh/settings/#setting-blockexoticsubdeps)
setting fails the install with an error like:

```text
registry error for xlsx: uses exotic specifier "https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz"
which is blocked by blockExoticSubdeps (declared by @gmickel/gno)
```

Check where that dependency comes from, then allow it by name with
`allow_exotic_deps`:

```toml
[tools]
"npm:@gmickel/gno" = { version = "2.3.0", allow_exotic_deps = ["xlsx"] }
```

mise writes the list to the install's `.config/aube/config.toml` as
`blockExoticSubdepsExclude`, so every other package is still checked, including
ones a later version adds. `allow_exotic_deps = true` exempts the whole graph,
future additions included, so prefer the list. If you can, ask the upstream
project to depend on a registry release instead. The option applies to embedded
aube and `aube_cli`; npm, pnpm and Bun do not enforce this check.

### Socket security scanner {#socket-security}

<span id="bun-compatible-security-scanner"></span>

Embedded aube implements
[Bun's Security Scanner API](https://bun.sh/docs/pm/security-scanner-api) and
works with Socket's
[`@socketsecurity/bun-security-scanner`](https://socket.dev/blog/socket-integrates-with-bun-1-3-security-scanner-api).
Set `AUBE_SECURITY_SCANNER` to enable it:

```sh
MISE_NPM_PACKAGE_MANAGER=aube \
AUBE_SECURITY_SCANNER=/absolute/path/to/scanner.mjs \
  mise install npm:prettier@latest
```

Setting `MISE_NPM_PACKAGE_MANAGER=aube` makes sure the scanner runs even if your
settings select npm, Bun or pnpm.

The scanner runs after dependency resolution and before package tarballs are
downloaded. It receives the resolved direct and transitive registry packages,
and a fatal finding blocks the install. A configured scanner also fails closed
if it cannot start or finish. See
[aube's security scanner documentation](https://aube.sh/package-manager/security-scanner)
for its configuration.

mise installs each `npm:` tool in a separate project directory, so a bare
scanner package name does not resolve from that project's `node_modules`. Point
the variable at an absolute module path instead. For example, install the Socket
scanner in a separate, stable directory and put this wrapper next to that
directory's `node_modules`:

```js
// scanner.mjs
export { scanner } from "@socketsecurity/bun-security-scanner";
```

The scanner bridge needs Node.js 22.6 or newer. It inherits Socket variables
such as `SOCKET_SECURITY_API_KEY`, while aube removes common npm and GitHub
credentials from the scanner's environment.

### Socket Firewall

[Socket Firewall](https://docs.socket.dev/docs/socket-firewall-free) can
instead wrap mise itself:

```sh
sfw mise install npm:prettier@latest
sfw mise use -g npm:prettier
```

This works at the network layer. mise's npm metadata client and embedded aube
both use aube-registry, which honors `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY`
and loads the `NODE_EXTRA_CA_CERTS` bundle. Socket does not list mise or aube as
supported package managers, so Socket does not guarantee this setup.

## Choosing an installer

Use [`npm.package_manager`](/dev-tools/backends/npm.html#npm.package_manager) to select the installer:

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
[settings]
npm.package_manager = "pnpm"
```

An explicit installer takes precedence over `npm.shell_out` for installation.
Installer-specific options such as `pnpm_args` apply only to the selected
installer.

## Tool options

Set these on the tool's entry in `[tools]`, or inline, as in
`'npm:some-tool[allow_low_downloads=true]'`. Options every backend accepts are
described under [tool options](/dev-tools/#tool-options).

| Option                  | Installers                                          | What it does                                                                                                                         |
| ----------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `allow_builds`          | embedded aube, `aube_cli`, pnpm, npm 11.16 or later | Approves dependency build scripts; see [build scripts](#build-scripts-and-supply-chain-checks)                                       |
| `trust_policy_excludes` | embedded aube, `aube_cli`                           | Exempts reviewed packages from the trust policy; see [trust policy](#trust-policy)                                                   |
| `allow_low_downloads`   | embedded aube, `aube_cli`                           | Approves the requested package for reputation checks; see [low-download and new packages](#low-download-and-new-packages)            |
| `allow_exotic_deps`     | embedded aube, `aube_cli`                           | Allows dependencies from outside the registry; see [dependencies from outside the registry](#dependencies-from-outside-the-registry) |
| `aube_args`             | `aube_cli`                                          | Extra arguments for `aube add --global`                                                                                              |
| `pnpm_args`             | pnpm                                                | Extra arguments for `pnpm add --global`                                                                                              |
| `bun_args`              | Bun                                                 | Extra arguments for `bun install --global`                                                                                           |
| `npm_args`              | npm                                                 | Extra arguments for `npm install -g`                                                                                                 |
| `install_env`           | `aube_cli`, pnpm, Bun, npm                          | Environment variables for the installer; see below                                                                                   |

### `aube_args`, `pnpm_args`, `bun_args` and `npm_args`

<span id="aube-args"></span>
<span id="pnpm-args"></span>
<span id="bun-args"></span>
<span id="npm-args"></span>

Raw arguments added to the selected installer's command. Embedded aube ignores
`aube_args` with a warning. `npm_args` applies with `npm.package_manager = "npm"`,
or with `auto` and `npm.shell_out = true`. For example, to set pnpm's log level:

```toml
[tools]
"npm:some-tool" = { version = "latest", pnpm_args = "--loglevel=warn" }
```

### `install_env`

Set environment variables for an external installer (`aube_cli`, `pnpm`, `bun`
or `npm`). Embedded aube runs inside mise rather than as a separate process, so
it ignores `install_env` with a warning. For install-scoped aube settings, use
`allow_builds`, `allow_exotic_deps`, `allow_low_downloads` or
`trust_policy_excludes`; set anything else in mise's own environment.

```toml
[settings]
npm.package_manager = "npm"

[tools]
"npm:some-tool" = { version = "latest", install_env = { NODE_OPTIONS = "--max-old-space-size=4096" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="npm" :level="3" />

## Troubleshooting

| Symptom                                               | What to do                                                                                                             |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `node` is missing when the CLI starts                 | Add `node` to `[tools]`; the embedded installer does not add a runtime to your project.                                |
| A native dependency is missing                        | Its build script did not run. Approve only that package with [`allow_builds`](#build-scripts-and-supply-chain-checks). |
| Version listing works but installation fails          | Check which installer you selected and whether it can read the registry and credentials.                               |
| aube refuses a package for trust, downloads or source | Read the error, then the matching section under [supply-chain checks](#build-scripts-and-supply-chain-checks).         |

### Investigating trust downgrades

A `trustPolicy=no-downgrade` failure is a supply-chain signal, not an ordinary
version-resolution error. It means an earlier release had stronger npm
trusted-publisher, staged-publish or provenance evidence than the selected
release. Before you add an exception:

1. Inspect the npm release, its source tag or commit, the publisher identity
   and the tarball. Compare the metadata with npmjs.org and confirm nothing
   appears tampered with.
2. Check whether the maintainer published manually, backported outside the
   trusted workflow, skipped provenance, or used a registry that stripped
   metadata.
3. Report inconsistent evidence upstream. Release drift belongs with the
   package's maintainer; metadata present on npmjs.org but missing from a proxy
   or mirror belongs with that registry's operator.
4. Exempt only the reviewed version, as `"<package>@<version>"` in
   [`trust_policy_excludes`](#trust-policy). A bare package name exempts every
   future version.

With the default `auto` installer, `mise settings npm.shell_out=true` switches
to the npm CLI and skips this check entirely, so use it only as a last resort.
An explicit `npm.package_manager = "aube_cli"` still installs with standalone
aube and keeps the check.

Implementation: [`src/backend/npm.rs`](https://github.com/jdx/mise/blob/main/src/backend/npm.rs).
