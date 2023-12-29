---
---

# Getting Started

Installing rtx involves 2 steps:

1. Installing the CLI
2. [optional] Activating rtx or adding its shims to PATH

## macOS Quickstart

Below is the most common

### Install rtx CLI:

First we need to download the rtx CLI. See the sidebar for alternate methods such as [homebrew](./homebrew).
This directory is simply a suggestion. rtx can be installed anywhere.

```sh
$ mkdir -p ~/.local/share/rtx/bin
$ curl https://rtx.jdx.dev/rtx-latest-macos-arm64 > ~/.local/share/rtx/bin/rtx
$ chmod +x ~/.local/share/rtx/bin/rtx
$ ~/.local/share/rtx/bin/rtx --version
rtx 2024.x.x
```

::: tip
"~/.local/share/rtx/bin" does not need to be in PATH. rtx will automatically add its own directory to PATH when activated.
:::

### Activate rtx

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

### Test it out

Install a tool and set it as the global default:

```sh
$ rtx use --global node@20
$ node -v
v20.x.x
```
