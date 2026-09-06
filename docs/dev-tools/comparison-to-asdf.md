# Comparison to asdf

mise reads `.tool-versions` and supports [legacy asdf plugins](/dev-tools/backends/asdf.html).
You can start with an existing project's version declarations, then adopt
`mise.toml` for [environment variables](/environments/) and [tasks](/tasks/).
CLI and plugin compatibility are best-effort; mise has its own commands,
installation directories, and backend selection.

## Migrate from asdf to mise

Start in one project before changing your shell defaults:

1. [Install mise](/installing-mise.html).
2. From the project directory, run `mise config ls` and `mise ls --current` to
   inspect how mise reads the existing `.tool-versions`.
3. Run `mise install`, then verify a project command through mise:

   ```sh
   mise exec -- node --version
   ```

   Replace `node --version` with a command from your project's tools. mise uses
   its own installations; it does not automatically reuse asdf's install directory.

4. Once the project works, remove asdf activation and shim `PATH` entries from
   your shell startup files and [activate mise](/getting-started.html#activate-mise).
   Start a new shell and check `mise doctor` and `command -v node`.

Keep shared `.tool-versions` files if teammates still use asdf. To update one
with mise, specify the file and pin a concrete version:

```sh
mise use --path .tool-versions --pin node@24
```

Avoid mise-specific prefixes or backend identifiers in a file that asdf must read.
A `mise.toml` in the same directory takes precedence for tools it declares, so
check for conflicting declarations before keeping both files.

For personal defaults, use `mise use -g node@24` or edit
`~/.config/mise/config.toml`. Inspect `mise config ls` from outside a project to
see which home/global files are contributing to your environment. Copy the
versions you need explicitly instead of moving or rewriting your asdf installation.

## asdf in go (0.16+)

asdf 0.16 replaced the older Bash implementation with Go and changed parts of
its CLI. In particular, current asdf uses `asdf set` to write versions. See
[asdf's version commands](https://asdf-vm.com/manage/versions.html).
`mise set` has a different purpose: it writes environment variables. Use
`mise use` for tool versions.

## UX

`mise use` combines installation and configuration. For example:

```sh
mise use node@24 python@3.14
mise exec -- node --version
```

This records version requests for both tools and installs them if necessary.
After cloning a configured project, `mise install` installs its tools without
changing the declarations. You usually do not need to install plugins separately:
the registry selects a backend, and many tools use built-in backends.

## Command Compatibility

Prefer mise's documented command syntax in scripts. Some legacy asdf spellings
are accepted, but compatibility aliases are not a complete emulation of asdf.

| Goal                         | asdf command                 | mise command                           |
| ---------------------------- | ---------------------------- | -------------------------------------- |
| Install a specific version   | `asdf install nodejs 24.0.0` | `mise install node@24.0.0`             |
| Select a project version     | `asdf set nodejs 24.0.0`     | `mise use node@24.0.0` (also installs) |
| Select a personal default    | `asdf set -u nodejs 24.0.0`  | `mise use -g node@24.0.0`              |
| List available versions      | `asdf list all nodejs`       | `mise ls-remote node`                  |
| Inspect selected versions    | `asdf current`               | `mise ls --current`                    |
| Find the selected executable | `asdf which node`            | `mise which node`                      |
| Rebuild shims                | `asdf reshim`                | `mise reshim`                          |

mise recognizes the legacy tool names `nodejs` and `golang`, while its TOML
configuration uses the canonical names `node` and `go`.

## Performance

The main difference is when version selection runs. With normal `mise activate`,
mise updates `PATH` and environment variables at the shell prompt or supported
directory-change hooks. Subsequent tool calls use those executable paths directly.
asdf resolves a tool through a shim when it is called.

mise also provides shims for programs that need stable executable paths. Their
cost depends on how often commands pass through mise; use `mise exec -- <script>`
to prepare the environment once for a script and its child processes. Historical
benchmarks of Bash-based asdf do not describe current asdf performance. See
[shims](/dev-tools/shims.html) for the behavioral tradeoffs.

## Windows support

mise supports native Windows for compatible tools and backends. Availability
still depends on the tool's release artifacts and installation logic. Legacy
asdf shell plugins generally require a Unix environment; using mise does not
make those plugins native Windows installers. See [Windows](/installing-mise.html#windows-scoop).

## Supply chain security

An asdf plugin executes shell code during tool management. When using one through
either manager, you trust the plugin's maintainers in addition to the tool's
publisher. mise can install many tools through built-in download backends without
an external plugin.

## Security

Verification varies by distribution. For example, packslip verifies signed
manifests, and aqua supports verification methods described by its registry
entries. A tool or backend name alone is not a guarantee that a particular
artifact has a signature. See the [security guide](/security.html) for trust,
verification, and configuration controls.

## Extra backends

Use the registry shorthand when available, or select a package source explicitly:

```toml [mise.toml]
[tools]
node = "24"
ripgrep = "latest"
"npm:prettier" = "3"
```

Here the npm backend needs Node.js, so both are declared. Other backends can
install release binaries, Python CLIs, Rust crates, or private tools. See the
[backend reference](/dev-tools/backends/) for prerequisites and options.
