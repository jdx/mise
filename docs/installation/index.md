<details>
  <summary>Standalone</summary>
Note that it isn't necessary for `rtx` to be on `PATH`. If you run the activate script in your rc
file, rtx will automatically add itself to `PATH`.

```
curl https://rtx.jdx.dev/install.sh | sh
```

If you want to verify the install script hasn't been tampered with:

```
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 0x29DDE9E0
curl https://rtx.jdx.dev/install.sh.sig | gpg --decrypt > install.sh
# ensure the above is signed with the rtx release key
sh ./install.sh
```

or if you're allergic to `| sh`:

```
curl https://rtx.jdx.dev/rtx-latest-macos-arm64 > /usr/local/bin/rtx
```

It doesn't matter where you put it. So use `~/bin`, `/usr/local/bin`, `~/.local/share/rtx/bin/rtx`
or whatever.

Supported architectures:

- `x64`
- `arm64`

Supported platforms:

- `macos`
- `linux`

If you need something else, compile it with `cargo install rtx-cli` (see below).
[Windows isn't currently supported.](https://github.com/jdx/rtx/discussions/66)

</details>
<details>
  <summary>Homebrew</summary>

```
brew install rtx
```

Alternatively, use the custom tap (which is updated immediately after a release):

```
brew install jdx/tap/rtx
```

</details>
<details>
  <summary>MacPorts</summary>

```
sudo port install rtx
```

</details>
<details>
  <summary>Cargo</summary>

Build from source with Cargo:

```
cargo install rtx-cli
```

Do it faster with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```
cargo install cargo-binstall
cargo binstall rtx-cli
```

Build from the latest commit in main:

```
cargo install rtx-cli --git https://github.com/jdx/rtx --branch main
```

</details>
<details>
  <summary>npm</summary>

rtx is available on npm as a precompiled binary. This isn't a Node.js package—just distributed
via npm. This is useful for JS projects that want to setup rtx via `package.json` or `npx`.

```
npm install -g rtx-cli
```

Use npx if you just want to test it out for a single command without fully installing:

```
npx rtx-cli exec python@3.11 -- python some_script.py
```

</details>
<details>
  <summary>GitHub Releases</summary>

Download the latest release from [GitHub](https://github.com/jdx/rtx/releases).

```
curl https://github.com/jdx/rtx/releases/download/v2023.12.40/rtx-v2023.12.40-linux-x64 > /usr/local/bin/rtx
chmod +x /usr/local/bin/rtx
```

</details>
<details>
  <summary>apt</summary>

For installation on Ubuntu/Debian:

```
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://rtx.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/rtx-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/rtx-archive-keyring.gpg arch=amd64] https://rtx.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
sudo apt update
sudo apt install -y rtx
```

> [!IMPORTANT]
>
> If you're on arm64 you'll need to run the following:
>
> ```
> echo "deb [signed-by=/etc/apt/keyrings/rtx-archive-keyring.gpg arch=arm64] https://rtx.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
> ```

</details>
<details>
  <summary>dnf</summary>

For Fedora, CentOS, Amazon Linux, RHEL and other dnf-based distributions:

```
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://rtx.jdx.dev/rpm/rtx.repo
dnf install -y rtx
```

</details>
<details>
  <summary>yum</summary>

```
yum install -y yum-utils
yum-config-manager --add-repo https://rtx.jdx.dev/rpm/rtx.repo
yum install -y rtx
```

</details>
<details>
  <summary>apk</summary>

For Alpine Linux:

```
apk add rtx
```

_rtx lives in the [community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/rtx/APKBUILD)._

</details>
<details>
  <summary>aur</summary>

For Arch Linux:

```
git clone https://aur.archlinux.org/rtx.git
cd rtx
makepkg -si
```

</details>
<details>
  <summary>nix</summary>

For the Nix package manager, at release 23.05 or later:

```
nix-env -iA rtx
```

You can also import the package directly using
`rtx-flake.packages.${system}.rtx`. It supports all default Nix
systems.

</details>
<details>
  <summary>Docker</summary>

```
docker run jdxcode/rtx x node@20 -- node -v
```

</details>

### Register shell hook

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

> [!TIP]
>
> For homebrew and possibly other installs rtx is automatically activated so
> this is not necessary.
>
> See [`RTX_FISH_AUTO_ACTIVATE=1`](#rtx_fish_auto_activate1) for more information.

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
