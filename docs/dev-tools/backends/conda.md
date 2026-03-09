# Conda Backend <Badge type="warning" text="experimental" />

You may install packages directly from [conda-forge](https://conda-forge.org/) and other
Anaconda channels without needing conda or mamba installed.

This backend fetches pre-built packages from the anaconda.org API and extracts them directly,
making it a lightweight way to install conda packages as standalone CLI tools.

The code for this is inside the mise repository at [`./src/backend/conda.rs`](https://github.com/jdx/mise/blob/main/src/backend/conda.rs).

## Dependencies

None. Unlike other conda tools, this backend does not require conda, mamba, or micromamba
to be installed. It downloads and extracts packages directly from anaconda.org.

## Usage

The following installs the latest version of [ruff](https://anaconda.org/conda-forge/ruff)
and sets it as the active version on PATH:

```sh
$ mise use -g conda:ruff
$ ruff --version
ruff 0.8.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"conda:ruff" = "latest"
```

### Specifying a Version

```sh
mise use -g conda:ruff@0.7.0
```

### Using a Different Channel

By default, packages are installed from `conda-forge`. You can specify a different channel:

```sh
mise use -g "conda:ruff[channel=bioconda]"
```

Or in `mise.toml`:

```toml
[tools]
"conda:ruff" = { version = "latest", channel = "bioconda" }
```

## Platform Support

The conda backend automatically selects the appropriate package for your platform:

| Platform    | Conda Subdir  |
| ----------- | ------------- |
| Linux x64   | linux-64      |
| Linux ARM64 | linux-aarch64 |
| macOS x64   | osx-64        |
| macOS ARM64 | osx-arm64     |
| Windows x64 | win-64        |

If a platform-specific package is not available, the backend will fall back to `noarch` packages.

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="conda" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `conda` backend—these
go in `[tools]` in `mise.toml`.

### `channel`

Override the conda channel for a specific package:

```toml
[tools]
"conda:bioconductor-deseq2" = { version = "latest", channel = "bioconda" }
```

### `filter_bins`

Comma-separated list of binaries to symlink into a filtered `.mise-bins` directory.
This is useful when a conda package pulls in dependencies that shadow system commands
you don't want overridden on PATH.

For example, `conda:postgresql` installs not only `psql` but also hundreds of dependency
binaries like `clear`, `reset`, `tput`, `tabs`, and other ncurses utilities that shadow
standard system commands. With `filter_bins`, you can expose only the binaries you need:

```toml
[tools]
"conda:postgresql" = { version = "latest", filter_bins = "psql,pg_dump,pg_restore,createdb,dropdb" }
```

When enabled:

- A `.mise-bins` subdirectory is created inside the install path with symlinks only to the specified binaries
- Other binaries from the package and its dependencies are not exposed on PATH

## Common Channels

- `conda-forge` - Community-maintained packages (default)
- `bioconda` - Bioinformatics packages
- `nvidia` - NVIDIA CUDA packages

## Limitations

- Only installs single packages, not full conda environments with dependencies
- Best suited for standalone CLI tools that don't require complex dependency trees
- Does not manage Python environments or package dependencies like full conda/mamba
