# Getting started

By the end of this guide, you'll have a project with a managed tool, an environment
variable, and a task you can run. Shell activation is optional: the first examples
work without changing your shell configuration.

Already installed mise for an existing project? Review its `mise.toml`, then run
`mise install` from the project directory. Use `mise tasks ls` to see its tasks.

## 1. Install `mise` CLI {#installing-mise-cli}

See [installing mise](/installing-mise.html) for other ways to install mise (Homebrew, MacPorts, apt, Nix, and more).

::::tabs key:installing-mise
== Linux/macOS

```shell
curl https://mise.run | sh
```

The installer places the mise executable in `~/.local/bin`.

Verify the installation:

```shell
~/.local/bin/mise --version
# mise 2026.x.x
```

- `~/.local/bin` does not need to be in `PATH`. mise will automatically add its own directory to `PATH`
  when [activated](#activate-mise).

== Windows
::: code-group

```shell [scoop]
scoop install mise
```

```shell [winget]
winget install jdx.mise
```

```shell [chocolatey]
choco install mise
```

:::

== Debian/Ubuntu (apt)

```sh
sudo apt install -y extrepo
sudo extrepo enable mise
sudo apt update
sudo apt install -y mise
```

== Fedora 41+, RHEL/CentOS Stream 9+ (dnf)

```sh
sudo dnf copr enable jdxcode/mise
sudo dnf install mise
```

See the [copr page](https://copr.fedorainfracloud.org/coprs/jdxcode/mise/) for more information.

== Snap

```sh
sudo snap install mise --classic
```

See the [snapcraft.io page](https://snapcraft.io/mise) for more information.

::::

To customize where mise stores downloaded tools and other data, see
[directories](/directories.html).

## 2. Run your first tool {#mise-exec-run}

Use [`mise exec`](/cli/exec.html) to run a command with a specific tool version:

```sh
mise exec node@24 -- node --version
```

By default, mise downloads the tool if needed, then runs the command after `--`.
This does not add Node.js to your project configuration or change your current
shell's environment. The output starts with `v24.`; the patch version may vary.

::: tip
If `mise` isn't on `PATH` yet, use `~/.local/bin/mise` instead on macOS or Linux.
Activation in [step 4](#activate-mise) adds mise to your shell's `PATH`.
:::

## 3. Set up a project {#set-up-a-project}

For a fresh example, create a new directory. If you are using an existing
project, start in its root and skip the first two commands:

```sh
mkdir mise-example
cd mise-example
mise use node@24
```

[`mise use`](/cli/use.html) installs the tool and writes its version request to
`mise.toml`. Unlike `mise install`, it also changes your configuration.

### Set an environment variable {#environment-variables}

Edit the generated `mise.toml` to contain:

```toml [mise.toml]
[tools]
node = "24"

[env]
NODE_ENV = "development"
```

Run a command with both the configured tool and environment:

```sh
mise exec -- node -p process.env.NODE_ENV
# development
```

You can also [load variables from a `.env` file](/environments/#env-directives).

### Run a task {#run-a-task}

Add this section to the same `mise.toml`:

```toml [mise.toml]
[tasks.hello]
description = "Print the project's Node.js version and environment"
run = '''node -e "console.log(process.version, process.env.NODE_ENV)"'''
```

```sh
mise run hello
```

The output includes a Node.js version starting with `v24.` and `development`.
Tasks get the project's tools and environment automatically. By default,
`mise run` installs missing configured tools before running the task.

Commit `mise.toml` so teammates and CI can use the same configuration. The version
request `"24"` selects a release in the Node.js 24 series; it is not an exact pin.
See [lockfiles](/dev-tools/mise-lock.html) to share resolved versions across machines.

### Project configuration or global defaults?

| Command                       | What it does                                                                                              |
| ----------------------------- | --------------------------------------------------------------------------------------------------------- |
| `mise use node@24`            | Installs Node.js and saves the version request in the project config. Run it from your project directory. |
| `mise use --global node@24`   | Installs Node.js and saves a personal default in the global config.                                       |
| `mise install`                | Installs tools already declared in your configuration.                                                    |
| `mise exec -- node --version` | Runs one command with the project's tools and environment.                                                |
| `mise run hello`              | Runs a named task with the project's tools and environment.                                               |

Project configuration can override global defaults. Use `mise config ls` to see
which files are active and `mise ls --current` to inspect the selected tools.

### Trusting config files {#trust}

Review configuration from other people before running it: tasks, hooks, and some
environment directives can execute code. Use `mise trust` to explicitly trust a
config you've reviewed.

In normal mode, commands that execute project behavior, including `mise install`,
`mise exec`, and `mise run`, automatically trust the active config. With
[paranoid mode](/paranoid.html), non-global configs require explicit trust.
See [`mise trust`](/cli/trust.html) for details.

### Confirm what is active

```sh
mise config ls
mise ls --current
mise tasks ls
mise exec -- node --version
```

Use these from the project directory to check configuration discovery, tool
selection, task discovery, and command execution before adding shell activation.

## 4. Activate `mise` <Badge text="optional" /> {#activate-mise}

`mise exec` works great for one-off commands, but for interactive shells you'll probably want to activate mise so tools and environment variables are loaded automatically.

There are two approaches:

- [`mise activate`](/cli/activate) — updates your `PATH` and environment every time your prompt runs. Recommended for interactive shells.
- [Shims](/dev-tools/shims.html) — command entry points that select tool versions. Useful for editors and other programs that do not load your shell config. [Shims don't support all features of `mise activate`](/dev-tools/shims.html#shims-vs-path).

You can skip both and use `mise exec` or `mise run` to load the project environment
explicitly, including in CI and scripts.

Choose the instructions for your installation method and shell. Add the
activation line once; repeating an append command can create duplicate hooks.
For custom zsh or fish configuration locations, use your shell's actual config
path instead of the default shown below.

::::tabs key:activating-mise

== mise.run installer

::: code-group

```sh [bash]
echo 'eval "$(~/.local/bin/mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(~/.local/bin/mise activate zsh)"' >> ~/.zshrc
```

```sh [fish]
mkdir -p ~/.config/fish
echo '~/.local/bin/mise activate fish | source' >> ~/.config/fish/config.fish
```

:::

== Brew

::: code-group

```sh [bash]
echo 'eval "$(mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc
```

```sh [fish]
# do nothing! mise is automatically activated when using brew and fish
# you can disable this behavior with `set -Ux MISE_FISH_AUTO_ACTIVATE 0`
```

:::

== Windows

Add the following to your PowerShell profile (`$PROFILE`):

```powershell
(&mise activate pwsh) | Out-String | Invoke-Expression
```

If you need to open your PowerShell profile:

```powershell
# create profile if it doesn't already exist
if (-not (Test-Path $PROFILE)) {
    New-Item -ItemType Directory -Force (Split-Path -Parent $PROFILE) | Out-Null
    New-Item -ItemType File -Path $PROFILE | Out-Null
}
# open the profile
Invoke-Item $profile
```

- If not using PowerShell, add `<homedir>\AppData\Local\mise\shims` to `PATH`.

== Other package managers

::: code-group

```sh [bash]
echo 'eval "$(mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc
```

```sh [fish]
mkdir -p ~/.config/fish
echo 'mise activate fish | source' >> ~/.config/fish/config.fish
```

:::

::::

Restart your shell session after modifying your rc file. Run [`mise dr|doctor`](/cli/doctor.html) to verify everything is set up correctly.

With mise activated, tools are available directly on `PATH`:

```sh
mise use --global node@24
node -v
# v24.x.x
```

When you ran `mise use --global node@24`, mise updated your global config:

```toml [~/.config/mise/config.toml]
[tools]
node = "24"
```

## 5. Find more tools {#tool-backends}

Use the [registry](/registry.html) to find tool names such as `node`, `python`,
`jq`, and `ripgrep`. Most of the time, the name is all you need:

```sh
mise use ripgrep
mise exec -- rg --version
```

A **backend** tells mise where to get a tool and how to install it. You can choose
one explicitly, including for tools without a registry shorthand:

```sh
mise exec github:BurntSushi/ripgrep -- rg --version
```

Some backends require another runtime or package manager. Check the
[backend guide](/dev-tools/backends/) before using a new ecosystem.

## 6. Next steps {#next-steps}

- **Keep working in a project:** follow the [walkthrough](/walkthrough.html) for configuration overrides, upgrades, and daily commands.
- **Write build and test commands:** see [tasks](/tasks/).
- **Use mise outside a terminal:** set up your [editor](/ide-integration.html) or [CI pipeline](/continuous-integration.html).
- **Set up a machine:** use [bootstrap](/bootstrap.html) for declared system packages, dotfiles, and services.

### Set up autocompletion {#autocompletion}

Enable [shell completions](/installing-mise.html#autocompletion) to complete tools,
versions, and task names.

### If something doesn't work

Run `mise doctor` to check your setup. If a tool works through `mise exec` but not
as a plain command, check [shell activation](#activate-mise) and restart your shell.
See [troubleshooting](/troubleshooting.html) for other common problems.

#### GitHub API rate limiting {#github-api-rate-limiting}

If an error reports GitHub API rate limiting, configure a
[GitHub token](/dev-tools/github-tokens.html).

### Shell Feature Compatibility {#shell-feature-compatibility}

Not all shells support every mise feature:

| Feature                         | Bash | Zsh | Fish | Nushell | Elvish | Xonsh | PowerShell |
| ------------------------------- | ---- | --- | ---- | ------- | ------ | ----- | ---------- |
| `mise activate`                 | Yes  | Yes | Yes  | Yes     | Yes    | Yes   | Yes        |
| `mise shell`                    | Yes  | Yes | Yes  | Yes     | Yes    | Yes   | Yes        |
| Shell aliases (`[shell_alias]`) | Yes  | Yes | Yes  | No      | No     | No    | No         |
| `chpwd` hook                    | Yes  | Yes | Yes  | Yes     | Yes    | Yes   | Yes        |

PowerShell's directory-change hook requires PowerShell 7 or newer. Other
activation behavior remains available on supported older PowerShell versions.
