# Tips & Tricks

Short recipes for common workflows. Each section links to the full guide when setup or
platform details matter. Start with [Getting Started](/getting-started.html) if you have not
yet configured a project.

## macOS Rosetta

For precompiled Intel tools on Apple Silicon, set [`MISE_ARCH`](/configuration/settings.html#arch)
to `x64`. Keep those installations separate from native arm64 tools, and use the same
directory and architecture overrides for installation and execution:

```sh
export MISE_DATA_DIR="$HOME/.local/share/mise-x64"
export MISE_ARCH=x64
mise install node@24
mise exec node@24 -- node --version
```

Run this in a dedicated shell session. The overrides remain active until you unset them or
close that shell. Rosetta must be installed to execute an Intel binary on Apple Silicon;
source builds may also require an Intel toolchain and dependencies.

If a backend needs the mise process itself to run as Intel, install a separate binary:

```sh
curl -fsSL https://mise.run -o /tmp/install-mise.sh
MISE_INSTALL_PATH="$HOME/.local/bin/mise-x64" MISE_INSTALL_ARCH=x64 sh /tmp/install-mise.sh
"$HOME/.local/bin/mise-x64" --version
```

Keep the separate `MISE_DATA_DIR` when using that executable too. See the relevant
[language guide](/core-tools.html) for compilation requirements.

## Shebang

You can specify a tool and its version in a shebang without first setting up
a `mise.toml`/`.tool-versions` config:

```javascript [script.js]
#!/usr/bin/env -S mise x node@24 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

Save this as `script.js`, run `chmod +x script.js`, then execute `./script.js`.
This requires mise on `PATH` and an `env` implementation supporting `-S`; native Windows
does not execute Unix shebangs. Shell activation is unnecessary. For a committed wrapper
with additional installation options, see [tool stubs](/dev-tools/tool-stubs.html).

## Bootstrap script

Generate and commit a wrapper that downloads mise on first use:

```sh
mise generate install-script --localize --write bin/mise
./bin/mise install
```

Commit `bin/mise` and ignore `.mise/`, where the localized wrapper stores its binary, tools,
and cache. The generated wrapper records a default mise version; regenerate it to update
that default. See [CI bootstrapping](/continuous-integration.html#bootstrapping) for version
overrides, cache layout, and an example pipeline.

## Project-local task entrypoints

If you want contributors to run project tasks without installing mise first, pair
[`mise generate install-script`](/cli/generate/install-script.html) with
[`mise generate task-stubs`](/cli/generate/task-stubs.html):

```sh
mkdir -p bin
mise generate install-script --localize --write bin/mise --windows
mise generate task-stubs --mise-bin ./bin/mise
./bin/test
```

Define a `test` task before running the example. Commit the generated entrypoints and ignore
`.mise/`. The task stubs behave like small project commands, while `bin/mise`
downloads and runs the pinned mise binary for the project.

The example includes `--windows` for contributors on Windows. Windows cannot execute a shebang script, so
`mise generate install-script --write ./bin/mise --windows` writes `bin/mise.cmd` alongside it, and Windows contributors
run `.\bin\mise.cmd`. The launcher downloads the standalone `mise.exe` for the release and checks it
against a checksum embedded when the script was generated, so it needs nothing beyond what Windows
already ships.

Task stubs get a `.cmd` launcher beside each stub for the same reason, so the Windows form of the
example above is `.\bin\test.cmd`. The default `.cmd` task launcher can be generated on any platform, but `cmd.exe` can alter
shell metacharacters in arguments. Generate `--windows-launcher exe` on Windows when exact
argument forwarding is required; see [task stubs](/cli/generate/task-stubs.html).

## Machine bootstrapping

Use [`mise bootstrap`](/bootstrap.html) to apply machine setup declared in configuration.
Start with a preview:

```sh
mise bootstrap --dry-run
mise bootstrap
mise bootstrap status
```

Choose the parts your machine needs: [packages](/bootstrap/packages/),
[repositories](/bootstrap/repos.html), [dotfiles](/dotfiles.html),
[shell activation](/bootstrap/shell.html), [macOS defaults](/bootstrap/macos-defaults.html),
[launchd](/bootstrap/launchd.html), or [systemd](/bootstrap/systemd.html).
The full guide explains phase ordering and host selection; do not copy declarations for
unrelated platforms into a workstation config just to try the command.

Hooks and a `bootstrap` task are ordinary commands and need their own idempotent behavior.
When adopting existing Homebrew casks, see [ownership and macOS privacy permissions](/bootstrap/packages/brew.html#macos-privacy-security-tcc)
before replacing application bundles.

## Zsh with Zinit {#installation-via-zsh-zinit}

If you use [Zinit](https://github.com/zdharma-continuum/zinit), install mise using a supported
[installation method](/installing-mise.html), then activate it after plugins that modify PATH:

```zsh
# ~/.zshrc, after your Zinit setup
eval "$(mise activate zsh)"
```

This keeps mise updates under its installer or package manager. Follow the
[Zsh completion instructions](/installing-mise.html#autocompletion) to add completions,
and avoid initializing `compinit` repeatedly across your plugin and completion setup.

## CI/CD

Commit the project tool configuration and use `mise exec` or `mise run` in CI.
See [Continuous integration](/continuous-integration.html) for provider examples,
locked installs, and caching.

### GitHub Actions

For a repository that declares Node in `mise.toml`:

```yaml
name: tools
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v3
      - run: mise exec -- node --version
```

## `mise set`

Instead of manually editing `mise.toml` to add env vars, you can use [`mise set`](/cli/set.html):

```sh
mise set NODE_ENV=production
```

## Using Tera to read unsupported version files

Some project-local version files are already supported as [idiomatic version files](https://mise.jdx.dev/configuration.html#idiomatic-version-files). For other version files, you can use Tera templates in `mise.toml` to read the file and assign the version to the appropriate tool.

For example, to use a `.hvm` file with a plain Hugo version:

```toml
[tools]
hugo = "{{ read_file(path=config_root ~ '/.hvm') | trim }}"
```

HVM also supports versions with an `/extended` suffix. In mise, Hugo and Hugo Extended are separate tools, so strip the suffix and use `hugo-extended` instead:

```toml
[tools]
hugo-extended = "{{ read_file(path=config_root ~ '/.hvm') | trim | replace(from='/extended', to='') }}"
```

Create `.hvm` with a version string before evaluating either example. `config_root` keeps
the path tied to the configuration when you invoke mise from a subdirectory. Choose one
Hugo variant for the project. See [Templates](/templates.html) for functions and filters.

## [`mise run`](/cli/run.html) shorthand

As long as the task name doesn't conflict with a mise-provided command, you can skip the `run` part:

```sh
mise test
```

::: warning
Don't do this inside scripts: mise may add a command in a future version that conflicts with your task.
:::

## Watch tasks while editing

[`mise watch`](/cli/watch.html) reruns tasks when files change. It uses
`watchexec`, which you can install globally with mise:

```sh
mise use -g watchexec@latest
mise watch test
```

Use `--restart` for long-running processes that should restart on changes:

```sh
mise watch --restart dev
```

## Share task catalogs

For projects with a lot of tasks,
[`task_config.includes`](/tasks/task-configuration.html#task_config.includes)
can load task definitions from additional directories, `tasks.toml` files, or
remote git repositories. Replace the example URL with a repository and ref you trust:

```toml
[task_config]
includes = [
  "mise-tasks",
  "tasks.toml",
  "git::https://github.com/myorg/shared-tasks.git//tasks?ref=v1.0.0",
]
```

Included `tasks.toml` files use the same shape as the `[tasks]` table without
the `[tasks.]` prefix.

## Reuse task definitions with templates

Experimental [task templates](/tasks/templates.html) let multiple tasks share
common tools, environment variables, and command defaults:

```toml
[settings]
experimental = true

[task_templates."node:test"]
tools = { node = "24", pnpm = "latest" }
run = "pnpm test"

[tasks.test]
extends = "node:test"
run = "pnpm test -- --watch=false"
```

This assumes `pnpm test -- --watch=false` is accepted by your project's test script.
Use a template when packages share defaults, then override commands or paths locally.

## Redact secrets from task output

If a task may echo secrets in CI logs, add `redactions` to the task or config.
Values of the listed environment variables are replaced with `[redacted]` in processed task output:

```toml
redactions = ["API_KEY", "PASSWORD"]
```

Glob patterns are also supported:

```toml
redactions = ["SECRETS_*"]
```

Raw or interactive output bypasses redaction, and child programs still receive the original
values. See [redaction](/environments/#redactions) for supported output and logging boundaries.

## Software verification

See [Security](/security.html#software-verification) for mise's software verification controls,
including aqua signatures, SLSA provenance, and GitHub artifact attestations.

## Minimum release age

See [Security](/security.html#minimum-release-age) for supply-chain delay controls, backend support,
and transitive dependency filtering behavior.

## [`mise up --bump`](/cli/upgrade.html)

Use `mise up --bump` to upgrade all software to the latest version and update `mise.toml` files. This keeps the same precision as before,
so if you had `node = "24"` and node 26 is the latest, `mise up --bump node` will change `mise.toml` to `node = "26"`.

## cargo-binstall

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) can download prebuilt Rust CLI
binaries instead of compiling them. With `cargo.binstall` enabled (the default), mise uses
it for `cargo:` tools when available. Not every crate has a compatible prebuilt release;
see the [Cargo backend](/dev-tools/backends/cargo.html) for fallback behavior.

```sh
mise use -g cargo-binstall
```

## [`mise cache clear`](/cli/cache.html)

Clear a tool's cached metadata when checking for a new release, for example
`mise cache clear node`. `mise cache path` shows the active cache directory. A full
`mise cache clear` also affects environment and task caches; see [Cache Behavior](/cache-behavior.html).

## [`mise en`](/cli/en.html)

`mise en` starts a **new shell** with the current project environment. Exit that shell to
return to your original session. It does not add directory-change updates by itself; your
new shell's startup files may still activate mise. Use `mise en -s "bash --norc"` when you
want to skip Bash's rc file.

## Auto-install when entering a project

In a normally activated shell, run installation when entering a trusted project:

```toml
[hooks]
enter = "mise i -q"
```

The hook can download tools and run installation scripts when you enter the directory.
Use explicit `mise install` instead if you prefer to choose when that work runs.

## [`mise tool [TOOL]`](/cli/tool.html)

Inspect a tool's selected backend, version requests, and installation information:

```sh
mise tool ripgrep
```

Use `mise registry ripgrep` to inspect registry choices and `mise which rg` to find the
executable selected for the current project.

## [`mise cfg`](/cli/config.html)

List loaded configuration files and their tools:

```sh
mise config
```

Use this when a value comes from an unexpected file. For precedence and the file commands
write to, see [configuration](/configuration.html). `mise cfg` is an alias.

## `mise.lock`

Resolve configured requests into a committed lockfile:

```sh
mise lock
mise install --locked
```

Locking records concrete versions and, where the backend supports it, artifact URLs and
checksums. `mise install --locked` checks that the lockfile can satisfy the configuration.
Use `mise lock --bump --dry-run` to preview a version refresh before applying it.

Backends differ in the metadata they can lock. For a custom HTTP download, configure a
[checksum source](/dev-tools/backends/http.html#checksum-url) when available. See
[lockfiles](/dev-tools/mise-lock.html) for platform coverage and strict validation; do not
uninstall every tool just to regenerate metadata.

## Lockfile URL Tracking (Avoiding Rate Limits)

For backends that record artifact URLs, a lockfile can avoid repeated release-asset lookups
on later installs. It does not contain the artifacts themselves and does not eliminate all
network or authentication requirements. Downloads, verification, private repositories, and
backend-specific operations can still require access.

See [GitHub Tokens](/dev-tools/github-tokens.html) for credentials and
[lockfile behavior](/dev-tools/mise-lock.html) for each backend's guarantees.
