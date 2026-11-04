---
description: "Install Erlang/OTP with mise from precompiled builds where available, or from source with kerl."
---

# Erlang

mise installs Erlang/OTP from a precompiled build when one exists for your
platform and compiles it from source with [kerl](https://github.com/kerl/kerl)
otherwise.

## Quick start

Install Erlang for the project, then print the OTP release without starting an
interactive shell:

```sh
mise use erlang@29
mise exec -- erl -noshell -eval 'io:format("~s~n", [erlang:system_info(otp_release)]), halt().'
# 29
```

Use `mise use -g erlang@29` for a personal default. For Elixir, see
[Elixir](/lang/elixir.html), which explains how to pick an Elixir build for
your OTP major.

## Choosing a version

`erlang@29` selects the newest 29.x release, `erlang@29.1.1` selects that
release, and `erlang@latest` selects the newest release. List the available
versions with `mise ls-remote erlang`. See
[version requests](/dev-tools/versions.html) for the full syntax.

## Version files

mise reads no Erlang-specific version file. Set the version in `mise.toml` or
`.tool-versions`.

## How mise installs Erlang {#kerl}

By default mise tries a precompiled build for your platform and compiles from
source with kerl when none is available:

| Platform                                                             | Default source                                                                             |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| macOS (x64, arm64)                                                   | Precompiled, from [erlef/otp_builds](https://github.com/erlef/otp_builds)                  |
| Windows (x64, x86)                                                   | Precompiled, from the [Erlang/OTP GitHub releases](https://github.com/erlang/otp/releases) |
| Ubuntu 20.04, 22.04 and 24.04 (x64, arm64), including GitHub Actions | Precompiled, from [Bob](https://github.com/hexpm/bob#erlang-builds) on `builds.hex.pm`     |
| Other Linux distributions, and musl systems such as Alpine           | Compiled from source with kerl                                                             |

Set [`erlang.compile`](/lang/erlang.html#erlang.compile) to `true` to always build from source,
or to `false` to fail instead of compiling when no precompiled build exists.
mise sets no Erlang environment variables; the install's `bin` directory goes
on `PATH`.

On Linux, mise reads `/etc/os-release` to find the Ubuntu release. On GitHub
Actions Ubuntu runners it reads the runner's `ImageOS` instead (`ubuntu20`,
`ubuntu22` or `ubuntu24`). Where no precompiled build applies,
[`mise lock`](/cli/lock.html) records the Erlang/OTP source archive, so every
machine that uses the lockfile compiles the same release. When `mise lock`
runs on macOS or Windows and resolves a Linux platform, it records Bob's
Ubuntu 24.04 build.

On Alpine and NixOS, [`all_compile`](/configuration/settings.html#all_compile)
currently defaults to `true`, so mise compiles Erlang without trying a
precompiled build. This default is deprecated and changes in mise 2027.8.0, but
the change does little for Erlang. mise has no precompiled Erlang for musl, so
Alpine always compiles. mise does not detect NixOS as an Ubuntu release, so
even after the change NixOS compiles unless you choose one of Bob's targets. On
NixOS with [nix-ld](https://github.com/Mic92/nix-ld), set `all_compile = false`
and [`erlang.precompiled_os`](/lang/erlang.html#erlang.precompiled_os) to use one of Bob's
builds (see the next section). With `erlang.compile = false`, an install with no
applicable precompiled build fails instead of compiling, so do not set it on
Alpine, or on NixOS without `erlang.precompiled_os`. To keep compiling, set
`all_compile = true` explicitly.

An installed plugin named `erlang` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

### Bob's Ubuntu builds on other distributions

Bob's builds target Ubuntu but may also run on other glibc-based Linux
distributions with compatible system libraries. Ubuntu 26.04 is not detected
automatically. Set [`erlang.precompiled_os`](/lang/erlang.html#erlang.precompiled_os) to
use one of Bob's targets:

```toml [mise.toml]
[settings.erlang]
precompiled_os = "ubuntu-24.04"
```

Accepted targets are `ubuntu-20.04`, `ubuntu-22.04`, `ubuntu-24.04` and
`ubuntu-26.04`.

Precompiled builds link to system libraries such as OpenSSL, ncurses, ODBC and
wxWidgets. A compatible glibc alone does not guarantee that every optional
Erlang application works. `mise.lock` records the selected target, so choose
one that works on every machine that uses the lockfile.

### Source builds with kerl

Source builds need kerl's
[build dependencies](https://github.com/kerl/kerl#building-erlangotp). mise
downloads kerl itself, fetches the Erlang/OTP source archive, and runs
`kerl build-install` with one make job per CPU. To pass configure flags, set
`KERL_CONFIGURE_OPTIONS` with `install_env`:

```toml [mise.toml]
[tools]
erlang = { version = "29", install_env = { KERL_CONFIGURE_OPTIONS = "--without-javac" } }
```

mise sets `KERL_BASE_DIR`, `KERL_DOWNLOAD_DIR` and `KERL_BUILD_BACKEND` itself,
so values for those in `install_env` are ignored.

## Tool options

Erlang has no Erlang-specific options. `install_env` reaches kerl source
builds, the install step of precompiled Linux builds, and `postinstall`
commands. Other generic options are described in
[tool options](/dev-tools/#tool-options).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="erlang" :level="3" />
