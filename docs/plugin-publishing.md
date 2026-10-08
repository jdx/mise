---
description: "Release a mise plugin from Git or as a signed archive, test the published revision, and show users how to pin it."
socialDescription: "Release a mise plugin from Git or as a signed archive, and show users how to pin it."
---

# Publishing plugins

You publish a mise plugin by putting it where users can fetch it: a Git
repository pinned by tag, or a signed packslip archive. Users then install it
with `mise plugins install` or list it under `[plugins]` in `mise.toml`.

The mise registry does not accept new tools backed by asdf or vfox plugins (see
the [backend acceptance tiers](/contributing/registry.html#backend-acceptance-tiers)),
so your repository URL is how people install your plugin. These steps apply to
Lua tool, backend, environment and package manager plugins. For shell-script
plugins, see [asdf plugins (legacy)](/asdf-legacy-plugins.html).

## Before you release {#publishing-checklist}

- `metadata.lua` sets `name` and `version`, and describes the plugin with
  `description`, `homepage` and `license`. See
  [metadata.lua](/plugin-lua-modules.html#metadata).
- The README shows the install command, a `[tools]` or `[env]` example, the
  platforms you support, the external programs the plugin runs, and the oldest
  mise version you tested.
- The plugin passes the [isolated test](#testing-before-publication) on every
  platform the README lists.

## Test before you publish {#testing-before-publication}

Test with mise directories of your own, so the plugin under test cannot replace
a plugin you have installed or change your global config.

### Test in an isolated mise {#automated-testing}

Run this from the plugin's directory. The subshell keeps the exports out of
your shell, and the `trap` deletes the temporary directory when it exits:

```sh
plugin_dir="$PWD"
(
  test_dir="$(mktemp -d)"
  trap 'rm -rf "$test_dir"' EXIT
  export MISE_CONFIG_DIR="$test_dir/config"
  export MISE_SYSTEM_CONFIG_DIR="$test_dir/system"
  export MISE_GLOBAL_CONFIG_FILE="$test_dir/global.toml"
  export MISE_DATA_DIR="$test_dir/data"
  export MISE_CACHE_DIR="$test_dir/cache"
  export MISE_STATE_DIR="$test_dir/state"
  export MISE_ENV_CACHE=0
  export MISE_YES=1
  mkdir -p "$test_dir/project"
  cd "$test_dir/project"
  mise plugins link test-plugin "$plugin_dir"
  # the checks for your plugin type go here
)
```

Unset other `MISE_*` variables your shell exports that change the result, such
as `MISE_SAFE` or `MISE_DISABLE_BACKENDS`. These directories isolate mise's
own state only: package manager plugins and the installers a plugin runs can
still change the machine.

Then run the checks for your plugin type. The names are placeholders:

| Plugin type     | Checks                                                                                                                                                                                                                                                                      |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tool            | `mise ls-remote test-plugin`, `mise use test-plugin@1.0.0`, `mise exec -- example --version`. Cover `PostInstall` and `EnvKeys`, and, if the plugin reads one, an idiomatic version file in a project with no `[tools]` entry for the tool and the tool listed in `idiomatic_version_file_enable_tools`. |
| Backend         | `mise ls-remote test-plugin:example`, `mise use test-plugin:example@1.0.0`, `mise exec -- example --version`                                                                                                                                                               |
| Environment     | Add `_.test-plugin = { ... }` under `[env]`, then check `mise env --json` or `mise exec -- printenv MY_VAR`.                                                                                                                                                               |
| Package manager | Follow the testing advice in [package manager plugins](/package-plugin-development.html).                                                                                                                                                                                  |

Test a fixed tool version rather than `latest`, so a new upstream release does
not change the result. The command after `mise exec --` must name the tool's
executable.

### Test the published revision {#manual-testing}

[`mise plugins link`](/cli/plugins/link.html) uses your working tree, including
uncommitted, untracked and ignored files that a release does not contain, so a
plugin can work when linked and fail when installed. Compare
`git ls-tree -r --name-only v1.2.3` with your working tree, then install the
tag itself in a fresh isolated directory and repeat the checks:

```sh
mise plugins install test-plugin 'https://github.com/your-org/my-plugin#v1.2.3'
```

Run these checks in CI on every OS the README lists.

## Tag a release

Set `version` in `metadata.lua` to the new release and commit it. Then tag that
commit and push the tag:

```sh
git tag -a v1.2.3 -m "v1.2.3"
git push origin v1.2.3
```

Users select a release by its tag (`#v1.2.3`). mise does not read
`PLUGIN.version` to decide what to install, and your plugin's version says
nothing about the versions of the tool it manages. In the release notes, give
the pinned install command and any configuration users must change.

## Tell users how to install it {#distribution-methods}

Put both forms in your README:

```toml
# mise.toml: everyone who runs `mise install` here gets the plugin
[plugins]
my-plugin = "https://github.com/your-org/my-plugin#v1.2.3"
```

```sh
mise plugins install my-plugin 'https://github.com/your-org/my-plugin#v1.2.3'
```

`#v1.2.3` pins a tag, and a full commit SHA pins a revision that cannot move;
mise rejects abbreviated SHAs. With no `#`, users get the default branch. A
plugin ref uses `#`, not the `@version` syntax of tool requests.

For a private repository, users install over SSH
(`git@github.com:your-org/my-plugin.git`) or over HTTPS with a Git credential
helper. Do not tell them to put a token in the URL, where it ends up in shell
history, `mise.toml` and the plugin's Git remote. When access fails, run
`git ls-remote <url>` before you debug the plugin.

GitHub serves a zip for every tag, so a GitHub-hosted plugin needs no extra
build step for users who install from an archive:

```sh
mise plugins install my-plugin https://github.com/your-org/my-plugin/archive/refs/tags/v1.2.3.zip
```

On a host without tag archives, publish one as a release asset:

```sh
git archive --format=zip --prefix=my-plugin/ --output=my-plugin-v1.2.3.zip v1.2.3
```

An archive install has no Git history, so `mise plugins update` skips it. To
update, users run `mise plugins install --force my-plugin <new-archive-url>`.
See [Plugins](/plugins.html) for every source users can install from.

## Publish a signed packslip archive {#packslip}

A vfox plugin can also ship as a signed [packslip](/dev-tools/backends/packslip.html)
release. mise then checks the publisher's signature and the archive's digest
before it installs or replaces the plugin, and needs no Git checkout. Users
name the version from the signed manifest:

```sh
mise plugins install vfox:my-plugin 'packslip:your-org/my-plugin#1.2.3'
```

```toml
[plugins]
"vfox:my-plugin" = "packslip:your-org/my-plugin#1.2.3"
```

mise accepts a plugin release when:

- the repository is on GitHub and the release includes the signed manifest
  `packslip.sigstore.json`;
- the manifest's artifact is a `tar.gz` archive that declares
  `extensions.mise.plugin = "vfox"`, names no OS, architecture or libc, and
  declares no executables and no host requirements;
- the archive has `metadata.lua` at its root and holds only regular files and
  directories: no symbolic or hard links, no `.git` directory, no absolute
  paths or `..` components, and no names containing `\` or `:`.

Declare the artifact in the repository's `packslip.toml`:

```toml
[[artifact]]
path = "dist/my-plugin.tar.gz"
format = "tar.gz"
portable = true
bin = []

[artifact.extensions.mise]
plugin = "vfox"
```

Build the archive without a top-level directory, listing the files and
directories the plugin has:

```sh
mkdir -p dist
git archive --format=tar HEAD metadata.lua hooks lib LICENSE | gzip -n > dist/my-plugin.tar.gz
```

Sign the manifest in CI with the packslip GitHub Action; see
[publishing tools for mise](/dev-tools/backends/packslip.html#why-publish-one)
and the [packslip publishing guide](https://packslip.dev/docs/publishing/). The
[vfox-bfs release workflow](https://github.com/jdx/vfox-bfs/blob/main/.github/workflows/release.yml)
builds the archive, signs it, installs the result on Linux and macOS, and
uploads `vfox-bfs.tar.gz` and `packslip.sigstore.json` to the GitHub release.

mise applies the packslip backend's release-age and signer checks to plugin
releases. It remembers the signer it accepted and refuses a release signed by
another, so sign every release from the same workflow; see
[signer changes](/dev-tools/backends/packslip.html#pinned-signers).

A plain `mise plugins update my-plugin` keeps an explicit version pin. When the
project pins the plugin under `[plugins]`, users move to a new release by
changing the version there and running
`mise plugins install --force vfox:my-plugin`. mise does not warn when an
installed packslip plugin differs from its `[plugins]` pin, so a
`mise plugins update` to another version leaves the two out of step without
notice. Users who installed the plugin from the command line run
`mise plugins update my-plugin#1.3.0`, or reinstall with the new version:

```sh
mise plugins install --force vfox:my-plugin 'packslip:your-org/my-plugin#1.3.0'
```

## Ship an update {#maintenance-and-updates}

How users move to a new release depends on how they installed the plugin. When
the project pins it under `[plugins]`, they change the ref there and reinstall
the plugin from it:

```toml
[plugins]
my-plugin = "https://github.com/your-org/my-plugin#v1.3.0"
```

```sh
mise plugins install --force my-plugin
```

A ref in `[plugins]` applies only when mise installs the plugin. On a machine
that already has the plugin, changing the ref alone leaves the old release in
place, and `mise plugins update my-plugin#v1.3.0` alone moves the checkout away
from the pin. Either way, every `mise install` warns about the mismatch until
the checkout and the pin agree.

When users installed the plugin from the command line, they run:

```sh
mise plugins update my-plugin#v1.3.0
```

A plain `mise plugins update my-plugin` does not move a plugin that is checked
out at a tag.

Updating a plugin does not reinstall the tool versions it already installed.
Test a new release against an existing install as well as a fresh one,
especially when you change bin paths or environment hooks. If you rename an
option or change a default, keep the old form working for a while and show the
replacement in the release notes.

## Host it under mise-plugins

The [mise-plugins](https://github.com/mise-plugins) organization hosts
community plugins. To discuss moving yours there, start a
[GitHub discussion](https://github.com/jdx/mise/discussions). Hosting there
does not add a registry shorthand.

## Security {#security-considerations}

A plugin runs with the user's permissions. Treat everything that reaches a
command as untrusted, including tool names, versions, paths and options: quote
it for the shell, or avoid the shell with the `http`, `file` and `archiver`
[modules](/plugin-lua-modules.html#module-index). Return a checksum from
`PreInstall` for every download. Keep credentials out of the repository, and
never print tokens or response bodies that may contain them. See
[Security](/security.html) for what config trust and lockfiles cover.

## Troubleshooting

- The plugin works when linked but fails when installed: see
  [Test the published revision](#manual-testing).
- The wrong version appears: tell apart `PLUGIN.version`, the plugin's Git ref
  (`mise plugins ls --urls`) and the tool version (`mise ls --current`).
- Installing the plugin fails with an authentication error: see the advice on
  private repositories in
  [Tell users how to install it](#distribution-methods).
