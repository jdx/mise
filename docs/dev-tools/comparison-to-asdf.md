---
description: "Move projects and personal defaults from asdf to mise, with command equivalents and the differences to expect."
---

# Migrating from asdf

mise reads `.tool-versions` files and can run
[asdf plugins](/dev-tools/backends/asdf.html), so you can move one project at
a time while teammates keep using asdf. mise has its own commands, install
directories, and backends, so check each project after you switch.

## Migrate a project {#migrate-from-asdf-to-mise}

Start in one project before you change your shell setup:

1. [Install mise](/installing-mise.html).
2. In the project directory, run `mise config ls` and `mise ls --current` to
   see how mise reads the existing `.tool-versions`.
3. Run `mise install`, then check a tool through mise:

   ```sh
   mise exec -- node --version
   ```

   Use a command from your project's tools in place of `node --version`. mise
   installs its own copies of each tool. It does not reuse asdf's install
   directories, and the two managers must not share one.

4. Once the project works, remove asdf's activation and its shim directory
   from your shell startup files, and [activate mise](/shell-setup.html). Start
   a new shell and check `mise doctor` and `command -v node`.

To move the project to `mise.toml`, preview the conversion, then write it:

```sh
mise generate config --tool-versions .tool-versions --dry-run
mise generate config --tool-versions .tool-versions
```

Delete `.tool-versions` afterwards unless asdf users still need it. Once every
project works, you can
[uninstall asdf](https://asdf-vm.com/manage/core.html#uninstall).

## Command equivalents {#command-compatibility}

| Task                         | asdf                                                     | mise                                                                             |
| ---------------------------- | -------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Install a version            | `asdf install nodejs 24.0.0`                             | `mise install node@24.0.0`                                                       |
| Select a project version     | `asdf set nodejs 24.0.0` (before 0.16: `asdf local`)     | `mise use node@24.0.0` (also installs)                                           |
| Select a personal default    | `asdf set -u nodejs 24.0.0` (before 0.16: `asdf global`) | `mise use -g node@24.0.0`                                                        |
| Add a plugin                 | `asdf plugin add nodejs`                                 | Not needed for registry tools; otherwise `mise plugins install <name> <git-url>` |
| List installed versions      | `asdf list nodejs`                                       | `mise ls node`                                                                   |
| List available versions      | `asdf list all nodejs`                                   | `mise ls-remote node`                                                            |
| Show the latest version      | `asdf latest nodejs`                                     | `mise latest node`                                                               |
| Inspect selected versions    | `asdf current`                                           | `mise ls --current`                                                              |
| Find the selected executable | `asdf which node`                                        | `mise which node`                                                                |
| Show an install directory    | `asdf where nodejs`                                      | `mise where node`                                                                |
| Run with selected versions   | `asdf exec node -v`                                      | `mise exec -- node -v`                                                           |
| Uninstall a version          | `asdf uninstall nodejs 24.0.0`                           | `mise uninstall node@24.0.0`                                                     |
| Rebuild shims                | `asdf reshim`                                            | `mise reshim`                                                                    |

asdf 0.16 rewrote asdf in Go and replaced `asdf local` and `asdf global` with
[`asdf set`](https://asdf-vm.com/manage/versions.html). `mise set` is
unrelated: it sets environment variables. Use `mise use` for tool versions.

mise accepts some asdf spellings, such as `mise install node 24.0.0` and the
tool names `nodejs` and `golang`, but it does not emulate asdf completely. Use
mise's own syntax in scripts, and the names `node` and `go` in `mise.toml`.

## Share `.tool-versions` with asdf users {#share-tool-versions}

Keep a shared `.tool-versions` while teammates still use asdf. To update it
with mise, name the file and pin a concrete version that asdf accepts:

```sh
mise use --path .tool-versions --pin node@24
```

Keep [mise-only requests](/dev-tools/versions.html#request-syntax) such as
`prefix:`, `sub-`, and backend names like `aqua:jqlang/jq` out of a file that
asdf reads. A `mise.toml` in the same directory takes precedence for the tools
it declares, so check for conflicting declarations before you keep both files.
Tools listed only in `.tool-versions` are not written to
[`mise.lock`](/dev-tools/mise-lock.html), and `--locked` does not check them.

## Personal defaults {#personal-defaults}

mise keeps personal defaults in `~/.config/mise/config.toml`. Choose the
versions you want and set them with `mise use -g`:

```sh
mise use -g node@24 python@3.14
```

mise also reads `~/.tool-versions`, because it looks for `.tool-versions` in
every parent directory. asdf's global versions therefore keep applying to
projects under your home directory until you move them into
`~/.config/mise/config.toml` and delete or trim that file. Run `mise config ls`
from your home directory to see both files.

Copy versions one by one rather than converting the file wholesale. Check
entries that list several versions or use aliases, and keep the old file until
every tool works.

## What works differently {#what-works-differently}

### When versions are selected {#performance}

With `mise activate`, mise updates `PATH` and environment variables at the
shell prompt and when you change directories, and later commands run the tool
executables directly. asdf resolves a tool through a shim each time you call
it. mise also has [shims](/dev-tools/shims.html#overview) for programs that
need a stable executable path; they resolve the version on every call, as
asdf's do.

### Plugins and security {#security}

An asdf plugin is a shell script that runs on your machine whenever you
install a tool, so you trust its maintainer as well as the tool's publisher.
mise installs most registry tools through built-in backends with no plugin.
Some backends also verify what they download: packslip checks a signed release
manifest, and aqua applies the checksum, cosign, SLSA, or attestation checks
its registry declares. Verification depends on the tool, so check
`mise tool <name>`. See [Security](/security.html).

### Windows {#windows-support}

mise runs natively on Windows for tools and backends that publish Windows
builds. asdf plugins are shell scripts and need a Unix environment; mise does
not make them work natively on Windows. See
[Installing mise](/installing-mise.html) for Windows setup.

### Tools without an asdf plugin {#extra-backends}

Use a registry name when one exists, or choose a package source explicitly:

```toml [mise.toml]
[tools]
node = "24"
ripgrep = "latest"
"npm:prettier" = "3"
```

Prettier needs Node.js at runtime, so both are declared. Other
[backends](/dev-tools/backends/) install release binaries, Python CLIs, Rust
crates, and private tools.
