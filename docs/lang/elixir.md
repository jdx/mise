---
description: "Install Elixir with mise from Hex's precompiled builds, paired with a compatible Erlang/OTP."
---

# Elixir

mise installs precompiled Elixir releases from Hex. Elixir runs on Erlang/OTP,
so install [Erlang](/lang/erlang.html) as well.

## Quick start

Declare Erlang and an Elixir build compiled for the same OTP major:

```sh
mise use erlang@29 elixir@1.20.4-otp-29
mise exec -- elixir --version
```

`elixir --version` reports both the Elixir and the Erlang/OTP version, which
helps diagnose a mismatched pair. When both tools are configured, mise installs
Erlang before Elixir.

In an existing Mix project, run `mise exec -- mix deps.get` to fetch the
application's dependencies; selecting Elixir does not install them. Add `-g` to
`mise use` to set personal defaults.

## Choosing a version

Hex publishes each Elixir release compiled for several Erlang/OTP majors, as
`X.Y.Z-otp-NN`. Pick the build whose suffix matches your Erlang major:

| Request                | Selects                                                     |
| ---------------------- | ----------------------------------------------------------- |
| `elixir@1.20.4-otp-28` | Elixir 1.20.4 compiled for OTP 28                           |
| `elixir@1.20.4`        | The Elixir 1.20.4 build without an OTP suffix               |
| `elixir@1.20`          | The newest 1.20.x, compiled for the newest OTP it supports  |
| `elixir@latest`        | The newest release, compiled for the newest OTP it supports |

A prefix such as `1.20` or `latest` picks the newest OTP variant. That only
matches your Erlang when Erlang is also on its newest major, so pinning Erlang
to an older major while Elixir stays on `latest` gives a mismatched pair. List
the builds with `mise ls-remote elixir 1.20`.

## Version files

mise can read `.exenv-version`. Enable it for Elixir:

```sh
mise settings add idiomatic_version_file_enable_tools elixir
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

## Hex, Rebar and Mix archives

Unless `MIX_HOME` or `MIX_ARCHIVES` is already set in your shell or in `[env]`,
mise sets them to directories inside the selected Elixir version:

| Variable       | Value                     |
| -------------- | ------------------------- |
| `MIX_HOME`     | `<install>/.mix`          |
| `MIX_ARCHIVES` | `<install>/.mix/archives` |

mise also puts `<install>/.mix/escripts` on `PATH`. As a result,
`mix archive.install`, `mix escript.install`, Hex and Rebar are per Elixir
version. After installing a new Elixir version, install Hex and Rebar again:

```sh
mise exec -- mix local.hex --force
mise exec -- mix local.rebar --force
```

To share them across versions, set both variables and put the shared escripts
directory on `PATH`. mise does not override a `MIX_HOME` or `MIX_ARCHIVES` that
your shell or `[env]` already sets:

```toml [mise.toml]
[env]
MIX_HOME = "{{env.HOME}}/.mix"
MIX_ARCHIVES = "{{env.HOME}}/.mix/archives"
_.path = ["{{env.HOME}}/.mix/escripts"]
```

## How mise installs Elixir

mise downloads the build's zip archive from `builds.hex.pm` and runs
`elixir --version` with the configured Erlang on `PATH` to check it.
[`mise lock`](/cli/lock.html) records the checksum Hex publishes for the build.

An installed plugin named `elixir` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

Elixir has no Elixir-specific options. Generic options such as `install_env`,
`postinstall` and `os` work as described in
[tool options](/dev-tools/#tool-options). `install_env` reaches only the
`elixir --version` check and `postinstall` commands; it does not set `MIX_HOME`
or other variables for later use. Set those in `[env]`.
