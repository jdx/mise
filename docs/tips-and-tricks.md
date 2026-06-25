# Tips & Tricks

An assortment of helpful tips for using `mise`.

## macOS Rosetta

If you have a need to run tools as x86_64 on Apple Silicon, this can be done with mise however you'll currently
need to use the x86_64 version of mise itself. A common reason for doing this is to support compiling node <=14.

You can do this either with the [`MISE_ARCH`](https://mise.en.dev/configuration/settings.html#arch)
setting or by using a dedicated rosetta mise bin as described below:

First, you'll need a copy of mise that's built for x86_64:

```sh
$ curl https://mise.run | MISE_INSTALL_PATH=~/.local/bin/mise-x64 MISE_INSTALL_ARCH=x64 sh
$ ~/.local/bin/mise-x64 --version
mise 2024.x.x
```

::: warning
If `~/.local/bin` is not in PATH, you'll need to prefix all commands with `~/.local/bin/mise-x64`.
:::

Now you can use `mise-x64` to install tools:

```sh
mise-x64 use -g node@20
```

## Shebang

You can specify a tool and its version in a shebang without needing to first
set up a `mise.toml`/`.tool-versions` config:

```typescript
#!/usr/bin/env -S mise x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where mise isn't activated
(such as a non-interactive session).

## Bootstrap script

You can download the <https://mise.run> script to use in a project bootstrap script:

```sh
curl https://mise.run > setup-mise.sh
chmod +x setup-mise.sh
./setup-mise.sh
```

::: tip
This file contains checksums so it's more secure to commit it into your project rather than
calling `curl https://mise.run` dynamically—though of course this means it will only fetch
the version of mise that was current when the script was created.
:::

## Project-local task entrypoints

If you want contributors to run project tasks without installing mise first, pair
[`mise generate bootstrap`](/cli/generate/bootstrap.html) with
[`mise generate task-stubs`](/cli/generate/task-stubs.html):

```sh
mkdir -p bin
mise generate bootstrap --localize --write bin/mise
mise generate task-stubs --mise-bin ./bin/mise
./bin/test
```

The generated task stubs behave like small project commands, while `bin/mise`
downloads and runs the pinned mise binary for the project.

## Machine bootstrapping <Badge type="warning" text="experimental" />

Beyond `[tools]`, mise can declare the rest of the machine setup needed for
a project or workstation, and [`mise bootstrap`](/cli/bootstrap.html)
converges it in one command — system packages, then repos, then dotfiles, then
shell activation, then macOS defaults, then LaunchAgents, then systemd user
services, then login shell, then tools, then a `bootstrap` task if you define
one:

```toml
[bootstrap.packages]                      # OS packages (apk/apt/dnf/pacman/brew)
"apk:build-base" = "latest"
"apt:build-essential" = "latest"
"brew:postgresql@17" = "latest"

[bootstrap.repos]                         # git repos cloned before dotfiles
"~/src/dotfiles" = { url = "git@github.com:jdx/dotfiles.git", ref = "main" }

[dotfiles]                             # dotfiles: symlink/copy/template
"~/.gitconfig" = { mode = "symlink" }
"~/.config/nvim" = { mode = "symlink" }

[bootstrap.mise_shell_activate]       # mise activation in shell startup files
zprofile = "shims"
zshrc = "activate"
fish = "activate"

[bootstrap.macos.dock]                 # friendly macOS defaults
autohide = true
orientation = "left"

[bootstrap.macos.finder]
show_pathbar = true

[bootstrap.macos.launchd.agents.my-sync]      # macOS user LaunchAgents
program = "~/.local/bin/my-sync"
run_at_load = true

[bootstrap.linux.systemd.units.my-sync]       # Linux systemd user services
exec_start = "~/.local/bin/my-sync --watch"
restart = "on-failure"

[bootstrap.user]                       # current user's login shell
login_shell = "/bin/zsh"

[bootstrap.hooks.post-defaults]        # optional phase hooks
run = "killall Dock || true"

[tasks.bootstrap]                      # anything else, with tools on PATH
run = "gh auth status || gh auth login"
```

```sh
mise bootstrap --yes   # new laptop or container -> ready to work
```

Everything is declarative and idempotent: re-running skips whatever is
already in its desired state, `mise bootstrap packages status --missing` and
`mise bootstrap dotfiles status --missing` make CI checks, and nothing is ever
applied implicitly. The exceptions are `[bootstrap.hooks]` and `[tasks.bootstrap]`,
which are imperative commands run during `mise bootstrap` and may have side
effects; treat hook commands as non-idempotent unless they are written to
converge safely. See
[Bootstrap](/bootstrap.html), [Bootstrap Packages](/bootstrap/packages/),
[Repos](/bootstrap/repos.html), [Dotfiles](/dotfiles.html),
[Shell Activation](/bootstrap/shell.html),
[macOS Defaults](/bootstrap/macos-defaults.html), [launchd](/bootstrap/launchd.html),
[systemd](/bootstrap/systemd.html), and [User Login Shell](/bootstrap/user.html).

## Installation via zsh zinit

[Zinit](https://github.com/zdharma-continuum/zinit) is a plugin manager for ZSH, which this snippet you will get mise (and usage for shell completion):

```sh
zinit as="command" lucid from="gh-r" for \
    id-as="usage" \
    atpull="%atclone" \
    jdx/usage
    #atload='eval "$(mise activate zsh)"' \

zinit as="command" lucid from="gh-r" for \
    id-as="mise" mv="mise* -> mise" \
    atclone="./mise* completion zsh > _mise" \
    atpull="%atclone" \
    atload='eval "$(mise activate zsh)"' \
    jdx/mise
```

## CI/CD

Using mise in CI/CD is a great way to synchronize tool versions for dev/build.

### GitHub Actions

mise is pretty easy to use without an action:

```yaml
jobs:
  build:
    steps:
      - run: |
          curl https://mise.run | sh
          echo "$HOME/.local/bin" >> $GITHUB_PATH
          echo "$HOME/.local/share/mise/shims" >> $GITHUB_PATH
```

Or you can use the custom action [`jdx/mise-action`](https://github.com/jdx/mise-action):

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: jdx/mise-action@v3
      - run: node -v # will be the node version from `mise.toml`/`.tool-versions`
```

## `mise set`

Instead of manually editing `mise.toml` to add env vars, you can use [`mise set`](/cli/set.html) instead:

```sh
mise set NODE_ENV=production
```

## Using Tera to read unsupported version files

Some project-local version files are already supported as [idiomatic version files](https://mise.en.dev/configuration.html#idiomatic-version-files). For other version files, you can use Tera templates in `mise.toml` to read the file and assign the version to the appropriate tool.

For example, to use a `.hvm` file with a plain Hugo version:

```toml
[tools]
hugo = "{{ read_file(path='.hvm') | trim }}"
```

HVM also supports versions with an `/extended` suffix. In mise, Hugo and Hugo Extended are separate tools, so strip the suffix and use `hugo-extended` instead:

```toml
[tools]
hugo-extended = "{{ read_file(path='.hvm') | trim | replace(from='/extended', to='') }}"
```

See [Templates](/templates.html) for more details on Tera functions and filters.

## [`mise run`](/cli/run.html) shorthand

As long as the task name doesn't conflict with a mise-provided command you can skip the `run` part:

```sh
mise test
```

::: warning
Don't do this inside of scripts because mise may add a command in a future version and could conflict with your task.
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
[`task_config.includes`](/tasks/task-configuration.html#task-config-includes)
can load task definitions from additional directories, `tasks.toml` files, or
remote git repositories:

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

This is especially useful in monorepos where each package needs similar build,
test, or lint tasks with small local overrides.

## Redact secrets from task output

If a task may echo secrets in CI logs, add `redactions` to the task or config.
The listed environment variables are replaced with `[redacted]` in task output:

```toml
redactions = ["API_KEY", "PASSWORD"]
```

Glob patterns are also supported:

```toml
redactions.env = ["SECRETS_*"]
```

## Software verification

See [Security](/security.html#software-verification) for mise's software verification controls,
including aqua signatures, SLSA provenance, and GitHub artifact attestations.

## Minimum release age

See [Security](/security.html#minimum-release-age) for supply-chain delay controls, backend support,
and transitive dependency filtering behavior.

## [`mise up --bump`](/cli/upgrade.html)

Use `mise up --bump` to upgrade all software to the latest version and update `mise.toml` files. This keeps the same semver range as before,
so if you had `node = "24"` and node 26 is the latest, `mise up --bump node` will change `mise.toml` to `node = "26"`.

## cargo-binstall

cargo-binstall is sort of like ubi but specific to rust tools. It fetches binaries for cargo releases. mise will use this automatically for `cargo:` tools if it is installed
so if you use `cargo:` you should add this to make `mise i` go much faster.

```sh
mise use -g cargo-binstall
```

## [`mise cache clear`](/cli/cache.html)

mise caches things for obvious reasons but sometimes you want it to use fresh data (maybe it's not noticing a new release). Run `mise cache clear` to remove the cache which
basically just run `rm -rf ~/.cache/mise/*`.

## [`mise en`](/cli/en.html)

`mise en` is a great alternative to `mise activate` if you don't want to always be using mise for some reason. It sets up the mise environment in your current directory
but doesn't keep running and updating the env vars after that.

## Auto-install when entering a project

Auto-install tools when entering a project by adding the following to `mise.toml`:

```toml
[hooks]
enter = "mise i -q"
```

## [`mise tool [TOOL]`](/cli/tool.html)

Get information about what backend a tool is using and other information with `mise tool [TOOL]`:

```sh
❯ mise tool ripgrep
Backend:            aqua:BurntSushi/ripgrep
Installed Versions: 14.1.1
Active Version:     14.1.1
Requested Version:  latest
Config Source:      ~/src/mise/mise.toml
Tool Options:       [none]
```

## [`mise cfg`](/cli/config.html)

List the config files mise is reading in a particular directory with `mise cfg`:

```sh
❯ mise cfg
Path                                    Tools
~/.config/mise/config.toml              (none)
~/.mise/config.toml                     (none)
~/src/mise.toml                         (none)
~/src/mise/.config/mise/conf.d/foo.toml (none)
~/src/mise/mise.toml                    actionlint, bun, cargo-binstall, cargo:…
~/src/mise/mise.local.toml              (none)
```

This is helpful figuring out which order the config files are loaded in to figure out which one is overriding.

## `mise.lock`

When lockfiles are enabled, mise will update `mise.lock` with full versions and tarball checksums (if supported by the backend).
These can be updated with [`mise up`](/cli/upgrade.html). You need to manually create the lockfile, then mise will add the tools to it:

```sh
touch mise.lock
mise i
```

The lockfile uses a consolidated format with `[tools.name.assets]` sections to organize asset information under each tool. Asset information includes checksums, file sizes, and optional download URLs. Legacy lockfiles with separate `[tools.name.checksums]` and `[tools.name.sizes]` sections are automatically migrated to the new format.

Note that at least currently mise needs to actually install the tool to get the tarball checksum (otherwise it would need to download the tarball just
to get the checksum of it since normally that gets deleted). So you may need to run something like `mise uninstall --all` first in order to have it
reinstall everything. It will store the full versions even if it doesn't know the checksum though so it'll still lock the version just not have a checksum
to go with it.

## Lockfile URL Tracking (Avoiding Rate Limits)

When you use a lockfile (`mise.lock`), mise stores the exact download URLs for each tool asset. This means that after the initial install, future `mise install` runs will use the URLs from the lockfile instead of making API calls to GitHub (or other providers). This has several benefits:

- **Avoids GitHub API rate limits**: No need to make repeated API calls for every install, which can quickly exhaust your rate limit, especially in CI or large teams.
- **No need for GITHUB_TOKEN**: Since the URLs are already known, you don’t need to set up a `GITHUB_TOKEN` for simple installs. See [GitHub Tokens](/dev-tools/github-tokens.html) for more on token configuration.
- **Faster installs**: Skipping API lookups speeds up repeated installs.

This is especially useful in CI/CD or when working in environments with strict network or authentication requirements.
