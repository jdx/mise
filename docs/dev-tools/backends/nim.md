# Nim Backend

You may install the [Nim](https://nim-lang.org) toolchain and its package manager
(`nimble`) as a mise-managed core tool — no external plugin is required.

The code for this is inside of the mise repository at
[`./src/plugins/core/nim.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/nim.rs).

## Dependencies

- **Windows (x86_64 / x86):** the official prebuilt zip is downloaded from
  nim-lang.org, so no extra tooling is required.
- **Linux (x86_64 / i686):** the official prebuilt tarball is downloaded from
  nim-lang.org, so no extra tooling is required.
- **Other platforms (macOS, Linux arm64, Windows arm64, …):** Nim is built
  from source, which requires `git` and a C compiler on PATH:
  `gcc`/`clang` on Unix (`build_all.sh`, the koch bootstrap) or mingw `gcc`
  on Windows (`build_all.bat`; Nim's csources bootstrap does not support
  MSVC).

You can force a source build on any platform with `nim.compile = true` (see
below), including Windows and Linux where prebuilts exist.

You can of course also install Nim some other way; mise will use whatever `nim`
is on PATH if you prefer not to use the core backend.

## Settings

- **`nim.compile`** (env: `MISE_NIM_COMPILE`) — controls source vs prebuilt:
  `true` always builds from source, `false` requires a prebuilt binary (and
  errors where none exists for the platform), and unset (the default) uses the
  prebuilt binary when available and builds from source otherwise.

## Usage

Install a specific version:

```sh
mise use -g nim@2.2.0
```

Then run the compiler or package manager:

```sh
$ mise x -- nim --version
Nim Compiler Version 2.2.0 ...
$ mise x -- nimble --version
nimble 0.14.0 ...
```

## Integration with the `nimble` backend

The `nimble` backend installs Nimble *packages*. It declares `nim` as an optional
dependency, so when both are present mise provisions the `nim` toolchain
automatically before building Nimble packages.
