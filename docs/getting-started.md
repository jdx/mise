<!-- markdownlint-disable MD034 -->

# Getting Started

This will show you how to install mise and get started with it. This is a suitable way when using an interactive shell like `bash`, `zsh`, or `fish`.

## 1. Install `mise` CLI

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
  when activated.

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

## 2. Activate `mise`

Now that `mise` is installed, you can optionally activate it or add its [shims](dev-tools/shims.md) to `PATH`.

- [`mise activate`](/cli/activate) method updates your environment variable and `PATH` every time your prompt is run to ensure you use the correct versions.
- [Shims](dev-tools/shims.md) are symlinks to the `mise` binary that intercept commands and load the appropriate environment

::: warning
Shims do not support all the features of `mise activate`.<br>
See [shims vs path](/dev-tools/shims.html#shims-vs-path) for more info.
:::

For interactive shells, `mise activate` is recommended. In non-interactive sessions, like CI/CD, IDEs, and scripts, using `shims` might work best. You can also not use any and call `mise exec/run` directly instead.
See [this guide](dev-tools/shims.md) for more information.

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

- When using `scoop`, mise is automatically activated
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

## 3. Using `mise`

:::info
Of course, if using mise solely for [environment management](/environments/)
or [running tasks](/tasks/)
this step is not necessary. You can use it to make sure `mise` is correctly setup.
:::

As an example, here is how you can install `node` and set it as the global default:

```sh
mise use --global node@22
```

You can now run `node` using `mise exec`:

```sh
mise exec -- node -v
# v22.x.x
```

:::tip
Use `mise x -- node -v` or set a shell alias in your shell's rc file like `alias x="mise x --"` to
save some keystrokes.
:::

If you did activate `mise` or add its shims to `PATH`, then `node` is also available directly!

```sh
node -v
# v22.x.x
```

Note that when you ran `mise use --global node@22`, `mise` updated the global `mise` configuration.

```toml [~/.config/mise/config.toml]
[tools]
node = "22"
```

Follow the [walkthrough](/walkthrough) for more examples on how to use mise.

::: warning
Many tools in mise require the use of the GitHub API. Unauthenticated requests to the GitHub API are
often rate limited. If you see 4xx errors while using mise, you can set `MISE_GITHUB_TOKEN` or `GITHUB_TOKEN`
to a token [generated from here](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) which
will likely fix the issue. The token does not require any scopes.
:::

### Set up the autocompletion

See [autocompletion](/installing-mise.html#autocompletion) to learn how to set up autocompletion for your shell.
