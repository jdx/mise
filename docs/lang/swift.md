---
description: "Install the swift.org Swift toolchains for macOS and Linux with mise and select one per project."
---

# Swift

mise installs the [Swift](https://swift.org/) toolchains published on
swift.org for macOS and Linux. Windows is not supported.

## Quick start

Install Swift for the current project and check the selected toolchain:

```sh
mise use swift@6
mise exec -- swift --version
```

Use `mise use -g swift@6` for a personal default. In an existing Swift package
with a `Package.swift`, run `mise exec -- swift build` to build it.

## Choosing a version

`swift@6` selects the newest 6.x release, `swift@6.4.0` selects that release,
and `swift@latest` selects the newest release. List the available versions with
`mise ls-remote swift`. See [version requests](/dev-tools/versions.html) for
the full syntax.

## Version files

mise can read `.swift-version`. Enable it for Swift:

```sh
mise settings add idiomatic_version_file_enable_tools swift
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

## Linux distributions

swift.org publishes Linux builds for specific releases of a few distribution
families: Ubuntu, Debian, Fedora, Amazon Linux and Red Hat UBI. mise picks the
build for your distribution's release, or the nearest older release in the
same family. If the family only has builds for newer releases, mise warns and
uses the oldest of them. If your family has no build at all, as on Arch Linux,
mise warns and installs the UBI build. Set [`swift.platform`](/lang/swift.html#swift.platform), for example
to `ubuntu24.04`, to choose a build explicitly. Every build links against
glibc, so musl systems such as Alpine are not supported.

`mise.lock` records which distribution build an entry describes, so an entry
written on Ubuntu does not apply on a Fedora machine, which installs the Fedora
build instead.

## How mise installs Swift

mise downloads the toolchain from `download.swift.org`. On Linux it verifies
the archive's OpenPGP signature ([`swift.gpg_verify`](/lang/swift.html#swift.gpg_verify)) and
extracts it. On macOS it expands swift.org's installer package with `pkgutil`
into mise's install directory; this toolchain is separate from the Swift that
ships with Xcode. mise then links the `swift*` and `sourcekit*` executables into
the install's `bin` directory and runs `swift --version` to check it. mise sets
no Swift environment variables.

## Troubleshooting

An installed plugin named `swift` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

### Missing shared libraries {#when-no-build-matches-your-distribution}

A build made for another distribution can ask for libraries under names your
distribution does not use. Arch Linux, for example, ships only the
wide-character ncurses (`libncursesw.so.6`), while the UBI build asks for
`libncurses.so.6`. The install then fails at the `swift --version` check, and
mise lists the libraries it could not find:

```text
this swift build needs shared libraries missing from this host: libform.so.6, libncurses.so.6, libpanel.so.6 (point LD_LIBRARY_PATH at them with install_env)
```

When the names differ but the libraries are compatible, create a directory of
aliases:

```sh
mkdir -p ~/.local/lib/curses-compat
ln -sf /usr/lib/libncursesw.so.6 ~/.local/lib/curses-compat/libncurses.so.6
ln -sf /usr/lib/libformw.so.6 ~/.local/lib/curses-compat/libform.so.6
ln -sf /usr/lib/libpanelw.so.6 ~/.local/lib/curses-compat/libpanel.so.6
```

Point the install at it with [`install_env`](/lang/swift.html#install-env), and point the
installed toolchain at it with `[env]`:

```toml [mise.toml]
[tools]
swift = { version = "6.4", install_env = { LD_LIBRARY_PATH = "{{env.HOME}}/.local/lib/curses-compat" } }

[env]
LD_LIBRARY_PATH = "{{env.HOME}}/.local/lib/curses-compat"
```

Alias a library only when the versions are compatible. If your distribution
lacks the library entirely, install it.

## Tool options

### `install_env`

Sets environment variables for the `swift --version` check mise runs after
installing, for `pkgutil` on macOS, and for `postinstall` commands. It does not
affect the download. Its main use is the library workaround in
[Missing shared libraries](#when-no-build-matches-your-distribution). Other
generic options are described in [tool options](/dev-tools/#tool-options).

## Further reading

- [A mise guide for Swift developers](https://tuist.dev/blog/2025/02/04/mise),
  from Tuist

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="swift" :level="3" />
