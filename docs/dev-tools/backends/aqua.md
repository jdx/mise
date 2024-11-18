# Aqua Backend <Badge type="warning" text="experimental" />

[Aqua](https://aquaproj.github.io/) tools may be used natively in mise. Aqua is encouraged as a backend for new tools if they
cannot be used with ubi as aqua tools directly fetch tarballs from the vendor without requiring unsafe
code execution in a plugin.

The code for this is inside the mise repository at [`./src/backend/aqua.rs`](https://github.com/jdx/mise/blob/main/src/backend/aqua.rs).

## Usage

The following installs the latest version of ripgrep and sets it as the active version on PATH:

```sh
$ mise use -g aqua:BurntSushi/ripgrep
$ rg --version
ripgrep 14.1.1
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"aqua:BurntSushi/ripgrep" = "latest"
```

Some tools will default to use aqua if they're specified in [registry.toml](https://github.com/jdx/mise/blob/main/registry.toml)
to use the aqua backend. To see these tools, run `mise registry | grep aqua:`.
