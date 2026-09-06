# Erlang

`mise` can be used to install and manage multiple versions of [erlang](https://www.erlang.org/) on the same system.

## Usage

Install Erlang for the project, then check the OTP release without starting an
interactive Erlang shell:

```sh
mise use erlang@latest
mise exec -- erl -noshell -eval 'io:format("~s~n", [erlang:system_info(otp_release)]), halt().'
```

Use `mise use -g erlang@latest` for a personal default, or replace `latest` with
the release series required by your application.

See available versions with `mise ls-remote erlang`.

These instructions use mise's built-in erlang support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/erlang.rs)
for backend details.

## kerl

mise tries a compatible precompiled build by default and falls back to a source
build when needed. Source builds use [kerl](https://github.com/kerl/kerl) and
require its platform build dependencies. Set `erlang.compile = true` to request
a source build, or `false` to fail when a precompiled build is unavailable.

See kerl's documentation for build dependencies and configuration.

On GitHub Actions Linux runners, `ImageOS=ubuntu24`, `ImageOS=ubuntu22`, and
`ImageOS=ubuntu20` map to their corresponding precompiled Erlang build targets. In the
default `erlang.compile` mode, unsupported values record the Erlang/OTP source archive
as the platform's locked input so installs can reproduce the kerl fallback.

The builds published by [Bob](https://github.com/hexpm/bob#erlang-builds) target Ubuntu,
but may also run on other glibc-based Linux distributions with compatible system
libraries. Set `erlang.precompiled_os` to opt in to one of Bob's Ubuntu targets:

```toml
[settings.erlang]
precompiled_os = "ubuntu-22.04"
```

Accepted targets are `ubuntu-20.04`, `ubuntu-22.04`, `ubuntu-24.04`, and
`ubuntu-26.04`.

Precompiled builds link to system libraries such as OpenSSL, ncurses, ODBC, and
wxWidgets. A compatible glibc version alone does not guarantee that every optional
Erlang application will work. The selected target is recorded in `mise.lock`; use a
target compatible with every machine that consumes the lockfile.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `erlang` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for kerl build/install commands and other install-time commands run by
the core `erlang` backend:

```toml
[tools]
erlang = { version = "latest", install_env = { KERL_CONFIGURE_OPTIONS = "--without-javac" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="erlang" :level="3" />
