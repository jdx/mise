# Flutter

`mise` can be used to install and manage multiple versions of [Flutter](https://flutter.dev/) on the
same system, including **custom builds installed directly from a git commit** (a fork or an internal
mirror).

> The following are instructions for using the flutter mise core plugin. This is used when there isn't
> a git plugin installed named "flutter".

The code for this is inside the mise repository at
[`./src/plugins/core/flutter.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/flutter.rs).

## Usage

Install an official stable release (downloaded as the prebuilt SDK archive):

```sh
mise use -g flutter@3.22.1   # install a specific stable version
mise use -g flutter@latest   # install the latest stable version
```

See available versions with `mise ls-remote flutter`.

## Custom versions (git commit / branch)

Unlike official releases — which are published as prebuilt archives — a custom build is installed by
cloning the Flutter git repository, checking out the requested ref, and running `flutter --version`
once to download the matching Dart SDK and engine artifacts.

```sh
# a specific commit
mise use -g flutter@ref:abcdef1234567890abcdef1234567890abcdef12

# a branch tip
mise use -g flutter@branch:master
mise use -g flutter@branch:beta
```

`ref:`, `branch:`, `tag:`, and `rev:` are all accepted (see
[tool version syntax](/dev-tools/#tool-version-syntax)).

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `flutter` backend.
These options go in the `[tools]` section in `mise.toml`.

### `remote`

Install from a fork or an internal mirror instead of `https://github.com/flutter/flutter.git`.
Setting `remote` (or its alias `url`) forces the git install path even for a plain version request, so
the version is interpreted as a git ref against your remote:

```toml
[tools]
flutter = { version = "ref:abcdef1234567890abcdef1234567890abcdef12", remote = "https://git.internal.example/flutter.git" }
```

### `install_env`

Set environment variables for install-time commands run by the core `flutter` backend:

```toml
[tools]
flutter = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
