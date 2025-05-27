# Installing Mise

If you are new to `mise`, follow the [Getting Started](/getting-started) guide first.

## Installation Methods

This page lists various ways to install `mise` on your system.

### <https://mise.run>

Note that it isn't necessary for `mise` to be on `PATH`. If you run the activate script in your
shell's rc
file, mise will automatically add itself to `PATH`.

```sh
curl https://mise.run | sh

# or with options
curl https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh
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

::: tip
As long as you don't change the version with `MISE_VERSION`, the install script will be pinned to whatever the latest
version was when it was downloaded with checksums inside the file. This makes downloading the file and putting it into
a project a great way to ensure that anyone installing with that script fetches the exact same mise bin.
:::

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
sudo apt update -y && sudo apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=amd64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

```sh [arm64]
sudo apt update -y && apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=arm64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
sudo apt update
sudo apt install -y mise
```

:::

### pacman

For Arch Linux:

```sh
sudo pacman -S mise
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

For Fedora 40, CentOS, Amazon Linux, RHEL and other dnf-based distributions:

```sh
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
dnf install -y mise
```

Fedora 41+ (dnf5)

```sh
dnf install -y dnf-plugins-core
dnf config-manager addrepo --from-repofile=https://mise.jdx.dev/rpm/mise.repo
dnf install -y mise
```

> [!NOTE]
> This repository maintains only the latest version of the mise CLI. Previous versions are removed and are not available once a new release is made.

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

### Windows - Scoop

This is the recommended way to install mise on Windows. It will automatically add your shims to PATH.

```sh
scoop install mise
```

### Windows - winget

```sh
winget install jdx.mise
```

### Windows - Chocolatey

::: info
chocolatey version is currently outdated.
:::

```sh
choco install mise
```

### Windows - manual

Download the latest release from [GitHub](https://github.com/jdx/mise/releases) and add the binary
to your PATH.

If your shell does not support `mise activate`, you would want to edit PATH to include the shims directory (by default: `%LOCALAPPDATA%\mise\shims`).

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

### Powershell

::: warning
See [about_Profiles](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_profiles) docs to find your actual profile location.
You will need to first create the parent directory if it does not exist.
:::

```powershell
echo 'mise activate pwsh | Out-String | Invoke-Expression' >> $HOME\Documents\PowerShell\Microsoft.PowerShell_profile.ps1
```

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

### Elvish

Add following to your `rc.elv`:

```shell
var mise: = (ns [&])
eval (mise activate elvish | slurp) &ns=$mise: &on-end={|ns| set mise: = $ns }
mise:activate
```

Optionally alias `mise` to `mise:mise` for seamless integration of `mise {activate,deactivate,shell}`:

```shell
edit:add-var mise~ {|@args| mise:mise $@args }
```

### Something else?

Adding a new shell is not hard at all since very little shell code is
in this project.
[See here](https://github.com/jdx/mise/tree/main/src/shell) for how
the others are implemented. If your shell isn't currently supported
I'd be happy to help you get yours integrated.

## Autocompletion

::: tip
Some installation methods automatically install autocompletion scripts.
:::

The [`mise completion`](/cli/completion.html) command can generate autocompletion scripts for your shell.
This requires `usage` to be installed. If you don't have it, install it with:

```shell
mise use -g usage
```

Then, run the following commands to install the completion script for your shell:

::: code-group

```sh [bash]
# This requires bash-completion to be installed
mkdir -p ~/.local/share/bash-completion/
mise completion bash --include-bash-completion-lib > ~/.local/share/bash-completion/completions/mise
```

```sh [zsh]
# If you use oh-my-zsh, there is a `mise` plugin. Update your .zshrc file with:
# plugins=(... mise)
# This must be after `source $ZSH/oh-my-zsh.sh` line in your .zshrc file.

# Otherwise, look where zsh search for completions with
echo $fpath | tr ' ' '\n'

# if you installed zsh with `apt-get` for example, this will work:
mkdir -p /usr/local/share/zsh/site-functions
mise completion zsh  > /usr/local/share/zsh/site-functions/_mise
```

```sh [fish]
mise completion fish > ~/.config/fish/completions/mise.fish
```

:::

Then source your shell's rc file or restart your shell.

## Uninstalling

Use `mise implode` to uninstall mise. This will remove the mise binary and all of its data. Use
`mise implode --help` for more information.

Alternatively, manually remove the following directories to fully clean up:

- `~/.local/share/mise` (can also be `MISE_DATA_DIR` or `XDG_DATA_HOME/mise`)
- `~/.local/state/mise` (can also be `MISE_STATE_DIR` or `XDG_STATE_HOME/mise`)
- `~/.config/mise` (can also be `MISE_CONFIG_DIR` or `XDG_CONFIG_HOME/mise`)
- on Linux: `~/.cache/mise` (can also be `MISE_CACHE_DIR` or `XDG_CACHE_HOME/mise`)
- on macOS: `~/Library/Caches/mise` (can also be `MISE_CACHE_DIR`)
