---
---

# Getting Started

Installing rtx involves 2 steps:

1. Installing the CLI
2. [optional] Activating rtx or adding its shims to PATH

## Quickstart

### 1. Install rtx CLI:

First we need to download the rtx CLI. See the sidebar for alternate methods such as [homebrew](./homebrew).
This directory is simply a suggestion. rtx can be installed anywhere.

::: code-group
```sh [macos-arm64]
$ mkdir -p ~/.local/share/rtx/bin
$ curl https://rtx.jdx.dev/rtx-latest-macos-arm64 > ~/.local/share/rtx/bin/rtx
$ chmod +x ~/.local/share/rtx/bin/rtx
$ ~/.local/share/rtx/bin/rtx --version
rtx 2024.x.x
```
:::

::: tip
"~/.local/share/rtx/bin" does not need to be in PATH. rtx will automatically add its own directory to PATH when activated.
:::

### 2. Activate rtx

`rtx activate` is one way to setup rtx but alternatively you can use [shims](./shims), [direnv](./direnv), or skip
this step entirely. If you skip it, then tools like `npm` and `node` will not be in PATH. You'll need to prefix
commands with `rtx exec` or run tasks with `rtx run` in order to use tools managed with rtx.

Make sure you restart your shell session after modifying your rc file in order for it to take effect.

::: code-group
```sh [bash]
echo 'eval "$(~/.local/share/rtx/bin/rtx activate bash)"' >> ~/.bashrc
```
```sh [zsh]
echo 'eval "$(~/.local/share/rtx/bin/rtx activate zsh)"' >> ~/.zshrc
```
```sh [fish]
echo '~/.local/share/rtx/bin/rtx activate fish | source' >> ~/.config/fish/config.fish
```
:::

### 2. (alternate) Add rtx shims to PATH

If you prefer to use shims, you can run the following to use rtx without activating it.
You can use .bashrc/.zshrc instead of .bash_profile/.zprofile if you prefer to only use
rtx in interactive sessions (.bash_profile/.zprofile will work in non-interactive places
like scripts or IDEs).

::: code-group
```sh [bash]
echo 'export PATH="$HOME/.local/share/rtx/shims:$PATH"' >> ~/.bash_profile
```
```sh [zsh]
echo 'export PATH="$HOME/.local/share/rtx/shims:$PATH"' >> ~/.zprofile
```
```sh [fish]
fish_add_path ~/.local/share/rtx/shims
```

:::info
rtx respects `RTX_DATA_DIR` and `XDG_DATA_HOME` if you'd like to change these locations.
:::

### Test it out

Install a tool and set it as the global default:

```sh
$ rtx use --global node@20
$ node -v
v20.x.x
```

If you did not activate rtx or add its shims to PATH, then you'll need to run the following:

```sh
$ rtx use --global node@20
$ rtx exec -- node -v
v20.x.x
```

:::tip
Use `rtx x -- node -v` or set a shell alias in your shell's rc file like `alias x="rtx x --"` to save some keystrokes.
:::
