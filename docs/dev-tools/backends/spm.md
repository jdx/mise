---
description: "Install Swift package executables from GitHub or GitLab, from an artifact bundle or a source build."
---

# spm backend

The `spm` backend installs command-line executables from
[Swift Package Manager](https://www.swift.org/documentation/package-manager)
packages hosted on GitHub or GitLab. It uses a prebuilt artifact bundle from the
release when one matches your platform, and otherwise builds the package from
source with `swift build`.

## Requirements {#dependencies}

This backend needs Swift, even for artifact bundles, because mise asks Swift for
the target triple. Install Swift [manually](https://www.swift.org/install) or
[with mise](/lang/swift.html). On macOS, the toolchain in Xcode works when Xcode
is selected with `xcode-select`. Source builds also need Git and the package's
own build dependencies.

## Usage

Install SwiftFormat in the current project, then run it:

```sh
mise use spm:nicklockwood/SwiftFormat
mise exec -- swiftformat --version
```

This writes the following to `mise.toml`. Add `-g` for a global tool.

```toml
[tools]
"spm:nicklockwood/SwiftFormat" = "latest"
```

SwiftFormat's releases include `swiftformat.artifactbundle.zip`. When the bundle
has an executable for your Swift target triple, mise installs it; otherwise mise
builds the package from source. Check the package's required Swift or Xcode
version before a source build.

A package can also ship only an artifact bundle:

```sh
mise use spm:giginet/swift-testing-revolutionary@0.4.0
mise exec -- swift-testing-revolutionary --help
```

Versions are release tags exactly as published, so
`mise ls-remote spm:owner/repo` shows `v1.2.0` for a repository that tags
`v1.2.0`. Both `@1.2.0` and `@v1.2.0` resolve to that tag.

### Supported syntax

| Form                                    | Example                                                      |
| --------------------------------------- | ------------------------------------------------------------ |
| GitHub shorthand                        | `spm:nicklockwood/SwiftFormat`                               |
| GitHub shorthand with a release version | `spm:nicklockwood/SwiftFormat@0.63.1`                        |
| GitHub URL                              | `spm:https://github.com/nicklockwood/SwiftFormat.git`        |
| GitHub URL with a release version       | `spm:https://github.com/nicklockwood/SwiftFormat.git@0.63.1` |
| GitLab URL                              | `spm:https://gitlab.com/owner/repo.git`                      |
| Self-hosted URL                         | `spm:https://git.example.com/owner/repo.git`                 |
| A specific commit                       | `spm:owner/repo@rev:<commit>`                                |
| A specific commit, by URL               | `spm:https://github.com/owner/repo.git@rev:<commit>`         |

A shorthand means GitHub unless the tool sets [`provider = "gitlab"`](#provider).
A `gitlab.com` URL always uses GitLab. For a self-hosted URL, mise derives the
API URL from the host; set `provider = "gitlab"` when the server runs GitLab.

A commit selector (`rev:<commit>`, or the equivalent `ref:<commit>`) always
builds from source. Use a full commit SHA for a reproducible installation.
Artifact bundles are release assets, so they cannot be combined with a commit
selector.

## Tool options

### `install_env` {#install-env}

Environment variables for the Swift commands mise runs, such as
`swift package dump-package`, `swift -print-target-info` and `swift build`. For
an artifact bundle install, they reach only `swift -print-target-info`; mise
downloads, extracts and links the bundle itself. On macOS, select the Xcode
developer directory for a system Swift toolchain:

```toml
[tools]
"spm:nicklockwood/SwiftFormat" = { version = "latest", install_env = { DEVELOPER_DIR = "/Applications/Xcode.app/Contents/Developer" } }
```

### `provider`

The forge that hosts the package: `github` (the default) or `gitlab`. Set it for
a GitLab shorthand or a self-hosted GitLab URL.

```toml
[tools]
"spm:patricklorran/ios-settings" = { version = "latest", provider = "gitlab" }
```

### `api_url` {#api-url}

The provider's API URL. For a self-hosted package, write the full package URL,
so that source builds clone from that server. mise derives the API URL from its
host, `https://<host>/api/v3` for GitHub or `https://<host>/api/v4` for GitLab;
set `api_url` when the API lives somewhere else:

```toml
[tools]
"spm:https://git.acme.com/acme/my-tool.git" = { version = "latest", provider = "gitlab", api_url = "https://git.acme.com/gitlab/api/v4" }
```

### `artifactbundle`

Whether to use SwiftPM artifact bundles. When it is unset, mise tries a matching
`*.artifactbundle.zip` release asset first and builds from source if none
matches. Set `artifactbundle = true` to require a bundle: the install fails when
no bundle matches the current Swift target triple. Set `artifactbundle = false`
to always build from source; it conflicts with the
[`spm.artifactbundle_only`](/configuration/settings.html#spm.artifactbundle_only) setting.

```toml
[tools]
"spm:giginet/swift-testing-revolutionary" = { version = "0.4.0", artifactbundle = true }
"spm:nicklockwood/SwiftFormat" = { version = "latest", artifactbundle = false }
```

### `artifactbundle_asset` {#artifactbundle-asset}

The artifact bundle to use when a release has several `*.artifactbundle.zip`
assets. Setting it also requires a bundle, as `artifactbundle = true` does.

```toml
[tools]
"spm:giginet/swift-testing-revolutionary" = { version = "0.4.0", artifactbundle_asset = "swift-testing-revolutionary.artifactbundle.zip" }
```

### `filter_bins` {#filter-bins}

The executable products to install. When it is unset, mise builds and links
every executable product declared in `Package.swift`, or links every matching
executable from an artifact bundle. Use it when a package ships helper
executables, such as test harnesses, that you do not want on `PATH`. For source
builds, mise filters before `swift build`, so it never builds the other
products.

The value is an array or a comma-separated string. The install fails when a
name does not match an executable product.

```toml
[tools]
"spm:swiftlang/swiftly" = { version = "latest", filter_bins = ["swiftly"] }
```

### `install_command` {#install-command}

A command to run in the checked-out package directory instead of discovering
executable products and running `swift build --product`. It runs with mise's
default inline shell and gets [`install_env`](#install-env), the Swift that mise
manages on `PATH`, and `PREFIX` and `MISE_TOOL_INSTALL_PATH` set to the install
directory. It applies
only to source builds and cannot be combined with `filter_bins`. mise never runs
a package's Makefile or install script unless you configure it here.

Use it for packages that install more than the executable, such as a dynamic
library or Swift modules placed by the package's own `make install`:

```toml
[tools]
"spm:owner/repo" = { version = "1.2.3", artifactbundle = false, install_command = "make install PREFIX=\"$MISE_TOOL_INSTALL_PATH\"" }
```

Some install scripts exit successfully even when `swift build` failed, so mise
checks that the command put at least one executable in `bin/` and fails the
install otherwise.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

<Settings child="spm" :level="3" />

## Troubleshooting

| Problem                               | What to check                                                                                                                                   |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| No matching artifact bundle           | The Swift target triple. Allow a source build only if the package supports your host and its build prerequisites are installed.                 |
| An unexpected source build            | Set `artifactbundle = true` so that a missing bundle fails the install.                                                                         |
| Several artifact bundles              | Set `artifactbundle_asset` to choose one.                                                                                                       |
| No executable products                | That the package publishes a command-line tool, the `filter_bins` names, or an `install_command` for a custom installation.                     |
| `latest` resolves to an unrelated tag | Repositories that release several products tag each one, and mise lists tags exactly as published. Pin a version, as in `spm:owner/repo@1.2.0`. |

Implementation: [`src/backend/spm.rs`](https://github.com/jdx/mise/blob/main/src/backend/spm.rs).
