<!-- markdownlint-disable MD034 -->

# Getting Started

This will show you how to install mise and get started with it. This is a suitable way when using an interactive shell like `bash`, `zsh`, or `fish`.

## 1. Install `mise` CLI {#installing-mise-cli}

See [installing mise](/installing-mise) for other ways to install mise (`macport`, `apt`, `yum`, `nix`, etc.).

:::tabs key:installing-mise
== Linux/macOS

```shell
curl https://mise.run | sh
```

By default, mise will be installed to `~/.local/bin` (this is simply a suggestion. `mise` can be installed anywhere).
You can verify the installation by running:

```shell
~/.local/bin/mise --version
# mise 2024.x.x
```

- `~/.local/bin` does not need to be in `PATH`. mise will automatically add its own directory to `PATH`
  when [activated](#activate-mise).

== Brew

```shell
brew install mise
```

== Windows
::: code-group

```shell [winget]
winget install jdx.mise
```

```shell [scoop]
# https://github.com/ScoopInstaller/Main/pull/6374
scoop install mise
```

```shell [chocolatey]
choco install mise
```

== Debian/Ubuntu (apt)

::: code-group

```sh [amd64]
sudo apt update -y && sudo apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=amd64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

```sh [arm64]
sudo apt update -y && sudo apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=arm64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

== Fedora (dnf)

```sh
sudo dnf install -y dnf-plugins-core
sudo dnf config-manager addrepo --from-repofile=https://mise.jdx.dev/rpm/mise.repo
sudo dnf install -y mise
```

:::

`mise` respects [`MISE_DATA_DIR`](/configuration) and [`XDG_DATA_HOME`](/configuration) if you'd like
to change these locations.

## 2. mise `exec` and `run` {#mise-exec-run}

Once `mise` is installed, you can immediately start using it. `mise` can be used to install and run [tools](/dev-tools/), launch [tasks](/tasks/), and manage [environment variables](/environments/).

The most essential feature `mise` provides is the ability to run [tools](/dev-tools/) with specific versions. A simple way to run a shell command with a given tool is to use [`mise x|exec`](/cli/exec.html). For example, here is how you can start a Python 3 interactive shell (REPL):

> _In the examples below, use `~/.local/bin/mise` (or the absolute path to `mise`) if `mise` is not already on `PATH`_

```sh
mise exec python@3 -- python
# this will download and install Python if it is not already installed
# Python 3.13.2
# >>> ...
```

or run node 22:

```sh
mise exec node@22 -- node -v
# v22.x.x
```

[`mise x|exec`](/cli/exec.html) is a powerful way to load the current `mise` context (tools & environment variables) without modifying your shell session or running ad-hoc commands with mise tools set. Installing [`tools`](/dev-tools/) is as simple as running [`mise u|use`](/cli/use.html).

```shell
mise use --global node@22 # install node 22 and set it as the global default
mise exec -- node my-script.js
# run my-script.js with node 22...
```

Another useful command is [`mise r|run`](/cli/run.html) which allows you to run a [`mise task`](/tasks/) or a script with the `mise` context.

:::tip
You can set a shell alias in your shell's rc file like `alias x="mise x --"` to save some keystrokes.
:::

## 3. Activate `mise` <Badge text="optional" /> {#activate-mise}

While using [`mise x|exec`](/cli/exec.html) is useful, for interactive shells, you might prefer to activate `mise` to automatically load the `mise` context (`tools` and `environment variables`) in your shell session. Another option is to use [shims](dev-tools/shims.md).

- [`mise activate`](/cli/activate) method updates your environment variable and `PATH` every time your prompt is run to ensure you use the correct versions.
- [Shims](dev-tools/shims.md) are symlinks to the `mise` binary that intercept commands and load the appropriate environment. Note that [**shims do not support all the features of `mise activate`**](/dev-tools/shims.html#shims-vs-path).

For interactive shells, `mise activate` is recommended. In non-interactive sessions, like CI/CD, IDEs, and scripts, using `shims` might work best. You can also not use any and call `mise exec/run` directly instead.
See [this guide](dev-tools/shims.md) for more information.

Here is how you can activate `mise` depending on your shell and the installation method:

:::tabs key:installing-mise

== https://mise.run

::: code-group

```sh [bash]
echo 'eval "$(~/.local/bin/mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(~/.local/bin/mise activate zsh)"' >> ~/.zshrc
```

```sh [fish]
echo '~/.local/bin/mise activate fish | source' >> ~/.config/fish/config.fish
```

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

== Windows

::: code-group

```powershell [powershell]
$shimPath = "$env:USERPROFILE\AppData\Local\mise\shims"
$currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$newPath = $currentPath + ";" + $shimPath
[Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
```

- If not using powershell, add `<homedir>\AppData\Local\mise\shims` to `PATH`.

== Other package managers

::: code-group

```sh [bash]
echo 'eval "$(mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc
```

```sh [fish]
echo 'mise activate fish | source' >> ~/.config/fish/config.fish
```

:::

Make sure you restart your shell session after modifying your rc file in order for it to take effect.
You can run [`mise dr|doctor`](/cli/doctor.html) to verify that mise is correctly installed and activated.

Now that `mise` is activated or its shims have been added to `PATH`, `node` is also available directly! (without using `mise exec`):

```sh
mise use --global node@22
node -v
# v22.x.x
```

Note that when you ran `mise use --global node@22`, `mise` updated the global `mise` configuration.

```toml [~/.config/mise/config.toml]
[tools]
node = "22"
```

## 4. Next steps {#next-steps}

Follow the [walkthrough](/walkthrough) for more examples on how to use mise.

### Set up the autocompletion {#autocompletion}

See [autocompletion](/installing-mise.html#autocompletion) to learn how to set up autocompletion for your shell.

### GitHub API rate limiting {#github-api-rate-limiting}

::: warning
Many tools in mise require the use of the GitHub API. Unauthenticated requests to the GitHub API are
often rate limited. If you see 4xx errors while using mise, you can set `MISE_GITHUB_TOKEN` or `GITHUB_TOKEN`
to a token [generated from here](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) which
will likely fix the issue. The token does not require any scopes.
:::
