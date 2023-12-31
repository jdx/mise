---
outline: 'deep'
---

# Getting Started

Using rtx typically involves 3 steps:

1. Installing the CLI
2. Activating rtx or adding its shims to PATH <Badge type="tip" text="optional" />
3. Adding tools to rtx

## Quickstart

### 1. Install rtx CLI:

First we need to download the rtx CLI.
See [below](#alternate-installation-methods) for alternate installation methods.
This directory is simply a suggestion.
rtx can be installed anywhere.

```sh
$ curl https://rtx.jdx.dev/install.sh | sh
$ ~/.local/share/rtx/bin/rtx --version
rtx 2024.x.x
```

::: tip
"~/.local/share/rtx/bin" does not need to be in PATH. rtx will automatically add its own directory to PATH when activated.
:::

### 2. Activate rtx <Badge type="tip" text="optional" />

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

### 2. Add rtx shims to PATH <Badge type="tip" text="Alternate" />

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
rtx respects [`RTX_DATA_DIR`](/configuration) and [`XDG_DATA_HOME`](/configuration) if you'd like to change these locations.
:::

### 3. Adding tools to rtx

:::info
Of course, if using rtx solely for [environment management](/environments) or [running tasks](/tasks/)
this step is not necessary.
:::

Install node and set it as the global default:

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
Use `rtx x -- node -v` or set a shell alias in your shell's rc file like `alias rx="rtx x --"` to save some keystrokes.
:::

## Alternate Installation Methods

### `install.sh`

Note that it isn't necessary for `rtx` to be on `PATH`. If you run the activate script in your shell's rc
file, rtx will automatically add itself to `PATH`.

```sh
curl https://rtx.jdx.dev/install.sh | sh
```

Options:
- `RTX_DEBUG=1` – enable debug logging
- `RTX_QUIET=1` – disable non-error output
- `XDG_DATA_HOME=/some/path` – change the data directory (default: `~/.local/share`)
- `RTX_DATA_DIR=/some/path` – change the rtx directory (default: `~/.local/share/rtx`)
- `RTX_INSTALL_PATH=/some/path` – change the binary path (default: `~/.local/share/rtx/bin`)

If you want to verify the install script hasn't been tampered with:

```sh
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 0x29DDE9E0
curl https://rtx.jdx.dev/install.sh.sig | gpg --decrypt > install.sh
# ensure the above is signed with the rtx release key
sh ./install.sh
```

or if you're allergic to `| sh`:

::: code-group
```sh [macos-arm64]
curl https://rtx.jdx.dev/rtx-latest-macos-arm64 > ~/.local/share/rtx/bin/rtx
```
```sh [macos-x64]
curl https://rtx.jdx.dev/rtx-latest-macos-x64 > ~/.local/share/rtx/bin/rtx
```
```sh [linux-x64]
curl https://rtx.jdx.dev/rtx-latest-linux-x64 > ~/.local/share/rtx/bin/rtx
```
```sh [linux-arm64]
curl https://rtx.jdx.dev/rtx-latest-linux-arm64 > ~/.local/share/rtx/bin/rtx
```
:::

It doesn't matter where you put it. So use `~/bin`, `/usr/local/bin`, `~/.local/share/rtx/bin/rtx`
or whatever.

Supported os/arch:

- `macos-x64`
- `macos-arm64`
- `linux-x64`
- `linux-x64-musl`
- `linux-arm64`
- `linux-arm64-musl`
- `linux-armv6`
- `linux-armv6-musl`
- `linux-armv7`
- `linux-armv7-musl`

If you need something else, compile it with `cargo install rtx-cli` (see below).
[Windows isn't currently supported.](https://github.com/jdx/rtx/discussions/66)

### Homebrew

```sh
brew install rtx
```

Alternatively, use the custom tap (which is updated immediately after a release):

```sh
brew install jdx/tap/rtx
```

### MacPorts

```sh
sudo port install rtx
```

### Cargo

Build from source with Cargo:

```sh
cargo install rtx-cli
```

Do it faster with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo install cargo-binstall
cargo binstall rtx-cli
```

Build from the latest commit in main:

```sh
cargo install rtx-cli --git https://github.com/jdx/rtx --branch main
```

### npm

rtx is available on npm as a precompiled binary. This isn't a Node.js package—just distributed
via npm. This is useful for JS projects that want to setup rtx via `package.json` or `npx`.

```sh
npm install -g rtx-cli
```

Use npx if you just want to test it out for a single command without fully installing:

```sh
npx rtx-cli exec python@3.11 -- python some_script.py
```

### GitHub Releases

Download the latest release from [GitHub](https://github.com/jdx/rtx/releases).

```sh
curl https://github.com/jdx/rtx/releases/download/v2023.12.40/rtx-v2023.12.40-linux-x64 > /usr/local/bin/rtx
chmod +x /usr/local/bin/rtx
```

### apt

For installation on Ubuntu/Debian:

```sh
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://rtx.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/rtx-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/rtx-archive-keyring.gpg arch=amd64] https://rtx.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
sudo apt update
sudo apt install -y rtx
```

::: warning
If you're on arm64 you'll need to run the following:

```
echo "deb [signed-by=/etc/apt/keyrings/rtx-archive-keyring.gpg arch=arm64] https://rtx.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
```
:::

### dnf

For Fedora, CentOS, Amazon Linux, RHEL and other dnf-based distributions:

```
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://rtx.jdx.dev/rpm/rtx.repo
dnf install -y rtx
```

### yum

```
yum install -y yum-utils
yum-config-manager --add-repo https://rtx.jdx.dev/rpm/rtx.repo
yum install -y rtx
```

### apk

For Alpine Linux:

```
apk add rtx
```

_rtx lives in the [community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/rtx/APKBUILD)._

### aur

For Arch Linux:

```
git clone https://aur.archlinux.org/rtx.git
cd rtx
makepkg -si
```

### nix

For the Nix package manager, at release 23.05 or later:

```
nix-env -iA rtx
```

You can also import the package directly using
`rtx-flake.packages.${system}.rtx`. It supports all default Nix
systems.

### Docker

```
docker run jdxcode/rtx x node@20 -- node -v
```


## Shells

#### Bash

```
echo 'eval "$(rtx activate bash)"' >> ~/.bashrc
```

#### Zsh

```
echo 'eval "$(rtx activate zsh)"' >> "${ZDOTDIR-$HOME}/.zshrc"
```

#### Fish

```
echo 'rtx activate fish | source' >> ~/.config/fish/config.fish
```

::: tip
For homebrew and possibly other installs rtx is automatically activated so
this is not necessary.

See [`RTX_FISH_AUTO_ACTIVATE=1`](/configuration#rtx_fish_auto_activate1) for more information.
:::

#### Nushell

```nushell
do {
  let rtxpath = ($nu.config-path | path dirname | path join "rtx.nu")
  run-external rtx activate nu --redirect-stdout | save $rtxpath -f
  $"\nsource "($rtxpath)"" | save $nu.config-path --append
}
```

#### Xonsh

Since `.xsh` files are [not compiled](https://github.com/xonsh/xonsh/issues/3953) you may shave a bit off startup time by using a pure Python import: add the code below to, for example, `~/.config/xonsh/rtx.py` config file and `import rtx` it in `~/.config/xonsh/rc.xsh`:

```xsh
from pathlib         import Path
from xonsh.built_ins import XSH

ctx = XSH.ctx
rtx_init = subprocess.run([Path('~/bin/rtx').expanduser(),'activate','xonsh'],capture_output=True,encoding="UTF-8").stdout
XSH.builtins.execx(rtx_init,'exec',ctx,filename='rtx')
```

Or continue to use `rc.xsh`/`.xonshrc`:

```xsh
echo 'execx($(~/bin/rtx activate xonsh))' >> ~/.config/xonsh/rc.xsh # or ~/.xonshrc
```

Given that `rtx` replaces both shell env `$PATH` and OS environ `PATH`, watch out that your configs don't have these two set differently (might throw `os.environ['PATH'] = xonsh.built_ins.XSH.env.get_detyped('PATH')` at the end of a config to make sure they match)

#### Something else?

Adding a new shell is not hard at all since very little shell code is
in this project.
[See here](https://github.com/jdx/rtx/tree/main/src/shell) for how
the others are implemented. If your shell isn't currently supported
I'd be happy to help you get yours integrated.

## Uninstalling

Use `rtx implode` to uninstall rtx. This will remove the rtx binary and all of its data. Use
`rtx implode --help` for more information.

Alternatively, manually remove the following directories to fully clean up:

- `~/.local/share/rtx` (can also be `RTX_DATA_DIR` or `XDG_DATA_HOME/rtx`)
- `~/.local/state/rtx` (can also be `RTX_STATE_DIR` or `XDG_STATE_HOME/rtx`)
- `~/.config/rtx` (can also be `RTX_CONFIG_DIR` or `XDG_CONFIG_HOME/rtx`)
- on Linux: `~/.cache/rtx` (can also be `RTX_CACHE_DIR` or `XDG_CACHE_HOME/rtx`)
- on macOS: `~/Library/Caches/rtx` (can also be `RTX_CACHE_DIR`)
