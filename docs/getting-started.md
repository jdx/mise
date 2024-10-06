# Getting Started

Using mise typically involves 3 steps:

1. Installing the CLI
2. Activating mise or adding its shims to PATH <Badge type="tip" text="optional" />
3. Adding tools to mise

## Quickstart

### 1. Install `mise` CLI

First we need to download the mise CLI.
See [below](#alternate-installation-methods) for alternate installation methods.
This directory is simply a suggestion.
mise can be installed anywhere.

```sh
$ curl https://mise.run | sh
$ ~/.local/bin/mise --version
mise 2024.x.x
```

::: tip
"~/.local/bin" does not need to be in PATH. mise will automatically add its own directory to PATH
when activated.
:::

### 2a. Activate mise

`mise activate` is one way to setup mise but alternatively you can
use [shims](dev-tools/shims.md), [direnv](./direnv), or skip
this step entirely. If you skip it, then tools like `npm` and `node` will not be in PATH. You'll
need to prefix
commands with `mise exec` or run tasks with `mise run` in order to use tools managed with mise.

Make sure you restart your shell session after modifying your rc file in order for it to take
effect. Also note that
this uses `~/.local/bin/mise` as the binary location since that's what <https://mise.run> uses by
default. If you've
installed mise by some other means it may be on PATH or somewhere different.

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

:::

### 2b. Alternative: Add mise shims to PATH

If you prefer to use shims, you can run the following to use mise without activating it.
You can use .bashrc/.zshrc instead of .bash_profile/.zprofile if you prefer to only use
mise in interactive sessions (.bash_profile/.zprofile will work in non-interactive places
like scripts or IDEs).

::: code-group

```sh [bash]
# note that bash will read from ~/.profile or ~/.bash_profile if the latter exists
# ergo, you may want to check to see which is defined on your system and only append to the existing file
echo 'export PATH="$HOME/.local/share/mise/shims:$PATH"' >> ~/.bash_profile
```

```sh [zsh]
echo 'export PATH="$HOME/.local/share/mise/shims:$PATH"' >> ~/.zprofile
```

```sh [fish]
fish_add_path ~/.local/share/mise/shims
```

:::tip
You can also run `mise activate --shims` which will do the above for you.
:::

:::info
mise respects [`MISE_DATA_DIR`](/configuration) and [`XDG_DATA_HOME`](/configuration) if you'd like
to change these locations.
:::

### 3. Adding tools to mise <Badge type="tip" text="optional" />

:::info
Of course, if using mise solely for [environment management](/environments)
or [running tasks](/tasks/)
this step is not necessary.
:::

Install node and set it as the global default:

```sh
$ mise use --global node@20
$ node -v
v20.x.x
```

If you did not activate mise or add its shims to PATH, then you'll need to run the following:

```sh
$ mise use --global node@20
$ mise exec -- node -v
v20.x.x
```

:::tip
Use `mise x -- node -v` or set a shell alias in your shell's rc file like `alias x="mise x --"` to
save some keystrokes.
:::

## Alternate Installation Methods

### <https://mise.run>

Note that it isn't necessary for `mise` to be on `PATH`. If you run the activate script in your
shell's rc
file, mise will automatically add itself to `PATH`.

```sh
curl https://mise.run | sh
```

Options:

- `MISE_DEBUG=1` – enable debug logging
- `MISE_QUIET=1` – disable non-error output
- `MISE_INSTALL_PATH=/some/path` – change the binary path (default: `~/.local/bin/mise`)
- `MISE_VERSION=v2024.5.17` – install a specific version

If you want to verify the install script hasn't been tampered with:

```sh
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 0x7413A06D
curl https://mise.jdx.dev/install.sh.sig | gpg --decrypt > install.sh
# ensure the above is signed with the mise release key
sh ./install.sh
```

or if you're allergic to `| sh`:

::: code-group

```sh [macos-arm64]
curl https://mise.jdx.dev/mise-latest-macos-arm64 > ~/.local/bin/mise
chmod +x ~/.local/bin/mise
```

```sh [macos-x64]
curl https://mise.jdx.dev/mise-latest-macos-x64 > ~/.local/bin/mise
chmod +x ~/.local/bin/mise
```

```sh [linux-x64]
curl https://mise.jdx.dev/mise-latest-linux-x64 > ~/.local/bin/mise
chmod +x ~/.local/bin/mise
```

```sh [linux-arm64]
curl https://mise.jdx.dev/mise-latest-linux-arm64 > ~/.local/bin/mise
chmod +x ~/.local/bin/mise
```

:::

It doesn't matter where you put it. So use `~/bin`, `/usr/local/bin`, `~/.local/bin` or whatever.

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

If you need something else, compile it with `cargo install mise` (see below).

### apk

For Alpine Linux:

```sh
apk add mise
```

_mise lives in
the [community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/mise/APKBUILD)._

### apt

For installation on Ubuntu/Debian:

::: code-group

```sh [amd64]
apt update -y && apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=amd64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

```sh [arm64]
apt update -y && apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=arm64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

:::

### aur

For Arch Linux:

```sh
git clone https://aur.archlinux.org/mise.git
cd mise
makepkg -si
```

### Cargo

Build from source with Cargo:

```sh
cargo install mise
```

Do it faster with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo install cargo-binstall
cargo binstall mise
```

Build from the latest commit in main:

```sh
cargo install mise --git https://github.com/jdx/mise --branch main
```

### dnf

For Fedora, CentOS, Amazon Linux, RHEL and other dnf-based distributions:

```sh
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
dnf install -y mise
```

### Docker

```sh
docker run jdxcode/mise x node@20 -- node -v
```

### Homebrew

```sh
brew install mise
```

### npm

mise is available on npm as a precompiled binary. This isn't a Node.js package—just distributed
via npm. This is useful for JS projects that want to setup mise via `package.json` or `npx`.

```sh
npm install -g @jdxcode/mise
```

Use npx if you just want to test it out for a single command without fully installing:

```sh
npx @jdxcode/mise exec python@3.11 -- python some_script.py
```

### GitHub Releases

Download the latest release from [GitHub](https://github.com/jdx/mise/releases).

```sh
curl -L https://github.com/jdx/mise/releases/download/v2024.1.0/mise-v2024.1.0-linux-x64 > /usr/local/bin/mise
chmod +x /usr/local/bin/mise
```

### MacPorts

```sh
sudo port install mise
```

### nix

For the Nix package manager, at release 23.05 or later:

```sh
nix-env -iA mise
```

You can also import the package directly using
`mise-flake.packages.${system}.mise`. It supports all default Nix
systems.

### yum

```sh
yum install -y yum-utils
yum-config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
yum install -y mise
```

### Windows

Download the latest release from [GitHub](https://github.com/jdx/mise/releases) and add the binary
to your PATH.

If your shell does not support `mise activate`, you would want to edit PATH to include the shims directory (by default: `%LOCALAPPDATA%\mise\shims`).

Note that Windows support is very minimal for now.

## Shells

### Bash

```sh
echo 'eval "$(mise activate bash)"' >> ~/.bashrc
```

### Zsh

```sh
echo 'eval "$(mise activate zsh)"' >> "${ZDOTDIR-$HOME}/.zshrc"
```

### Fish

```sh
echo 'mise activate fish | source' >> ~/.config/fish/config.fish
```

::: tip
For homebrew and possibly other installs mise is automatically activated so
this is not necessary.

See [`MISE_FISH_AUTO_ACTIVATE=1`](/configuration#mise_fish_auto_activate1) for more information.
:::

### Nushell

Nu
does [not support `eval`](https://www.nushell.sh/book/how_nushell_code_gets_run.html#eval-function)
Install Mise by appending `env.nu` and `config.nu`:

```nushell
'
let mise_path = $nu.default-config-dir | path join mise.nu
^mise activate nu | save $mise_path --force
' | save $nu.env-path --append
"\nuse ($nu.default-config-dir | path join mise.nu)" | save $nu.config-path --append
```

If you prefer to keep your dotfiles clean you can save it to a different directory then
update `$env.NU_LIB_DIRS`:

```nushell
"\n$env.NU_LIB_DIRS ++= ($mise_path | path dirname | to nuon)" | save $nu.env-path --append
```

### Xonsh

Since `.xsh` files are [not compiled](https://github.com/xonsh/xonsh/issues/3953) you may shave a
bit off startup time by using a pure Python import: add the code below to, for
example, `~/.config/xonsh/mise.py` config file and `import mise` it in `~/.config/xonsh/rc.xsh`:

```python
from pathlib         import Path
from xonsh.built_ins import XSH

ctx = XSH.ctx
mise_init = subprocess.run([Path('~/bin/mise').expanduser(),'activate','xonsh'],capture_output=True,encoding="UTF-8").stdout
XSH.builtins.execx(mise_init,'exec',ctx,filename='mise')
```

Or continue to use `rc.xsh`/`.xonshrc`:

```sh
echo 'execx($(~/bin/mise activate xonsh))' >> ~/.config/xonsh/rc.xsh # or ~/.xonshrc
```

Given that `mise` replaces both shell env `$PATH` and OS environ `PATH`, watch out that your configs
don't have these two set differently (might
throw `os.environ['PATH'] = xonsh.built_ins.XSH.env.get_detyped('PATH')` at the end of a config to
make sure they match)

### Something else?

Adding a new shell is not hard at all since very little shell code is
in this project.
[See here](https://github.com/jdx/mise/tree/main/src/shell) for how
the others are implemented. If your shell isn't currently supported
I'd be happy to help you get yours integrated.

## Uninstalling

Use `mise implode` to uninstall mise. This will remove the mise binary and all of its data. Use
`mise implode --help` for more information.

Alternatively, manually remove the following directories to fully clean up:

- `~/.local/share/mise` (can also be `MISE_DATA_DIR` or `XDG_DATA_HOME/mise`)
- `~/.local/state/mise` (can also be `MISE_STATE_DIR` or `XDG_STATE_HOME/mise`)
- `~/.config/mise` (can also be `MISE_CONFIG_DIR` or `XDG_CONFIG_HOME/mise`)
- on Linux: `~/.cache/mise` (can also be `MISE_CACHE_DIR` or `XDG_CACHE_HOME/mise`)
- on macOS: `~/Library/Caches/mise` (can also be `MISE_CACHE_DIR`)
