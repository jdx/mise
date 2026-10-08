---
description: "Add mise activation to bash, zsh, fish, PowerShell, Nushell, Xonsh, or Elvish, and install shell completions."
socialDescription: "Activate mise in bash, zsh, fish, PowerShell, Nushell, Xonsh, or Elvish."
---

# Activate mise in your shell

Shell activation hooks your interactive shell so that the current project's
tools are on `PATH` and its environment variables are set each time you change
directories or a prompt appears. Editors and other programs that never read your
shell's startup files can use shims instead, and scripts and CI can call
`mise exec` or `mise run` without either; [Shims](/dev-tools/shims.html#overview)
compares the three.

Add one activation line to the startup file of each shell you use, then open a
new terminal. The Bash, Zsh, Fish and PowerShell snippets check for the line
before appending it, so running one twice does not add a second hook. For
Nushell, Xonsh and Elvish, add the lines to the file yourself.

## Before you start {#before-you-start}

The activation line runs `mise` by name, so `mise` must be on `PATH` when the
startup file runs. The [mise.run installer](/installing-mise.html#mise-run)
puts mise at `~/.local/bin/mise`, which is not on `PATH` on every system. If
`command -v mise` prints nothing, write `~/.local/bin/mise` in place of `mise`
in the line you add. Once activated, mise adds its own directory to `PATH`.

## Bash {#bash}

```sh
activation='eval "$(mise activate bash)"'
grep -qxF "$activation" ~/.bashrc 2>/dev/null || printf '%s\n' "$activation" >> ~/.bashrc
```

Bash login shells, which macOS Terminal opens by default, read `~/.bash_profile`
and read `~/.bashrc` only when the profile sources it. If your `~/.bash_profile`
does not, add `[ -f ~/.bashrc ] && . ~/.bashrc` to it.

## Zsh {#zsh}

```sh
zshrc="${ZDOTDIR:-$HOME}/.zshrc"
activation='eval "$(mise activate zsh)"'
mkdir -p "$(dirname "$zshrc")"
grep -qxF "$activation" "$zshrc" 2>/dev/null || printf '%s\n' "$activation" >> "$zshrc"
```

The snippet writes to `$ZDOTDIR/.zshrc` when you set `ZDOTDIR`.

## Fish {#fish}

Run this in fish:

```fish
mkdir -p ~/.config/fish
set activation 'mise activate fish | source'
grep -qxF $activation ~/.config/fish/config.fish 2>/dev/null; or echo $activation >> ~/.config/fish/config.fish
```

Homebrew, and any package that installs mise's
`vendor_conf.d/mise-activate.fish`, activates mise in fish automatically, so you
can skip this step. To turn that off, set `MISE_FISH_AUTO_ACTIVATE` to `0`:

```fish
set -Ux MISE_FISH_AUTO_ACTIVATE 0
```

## PowerShell {#powershell}

`$PROFILE` is the profile of the current PowerShell host. This creates it if it
does not exist and adds the activation line once:

```powershell
if (-not (Test-Path $PROFILE)) {
    New-Item -ItemType File -Path $PROFILE -Force | Out-Null
}
$activation = '(&mise activate pwsh) | Out-String | Invoke-Expression'
if (-not (Select-String -Path $PROFILE -SimpleMatch $activation -Quiet)) {
    Add-Content $PROFILE $activation
}
```

Terminals, editors and PowerShell versions can each use a different profile;
see [PowerShell profiles](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_profiles).

mise updates the environment when you change directories only in PowerShell 7
and newer. Windows PowerShell 5.1 updates it at the next prompt and prints a
warning at startup; set `$env:MISE_PWSH_CHPWD_WARNING=0` to hide the warning.

## Nushell {#nushell}

Nushell loads activation as a generated module. Add this to `env.nu` (its path
is in `$nu.env-path`) so the module exists before `config.nu` is parsed:

```nushell
let mise_path = $nu.default-config-dir | path join mise.nu
^mise activate nu | save $mise_path --force
```

Add this to `config.nu` (its path is in `$nu.config-path`):

```nushell
use ($nu.default-config-dir | path join mise.nu)
```

Restart Nushell after saving both files. If `mise` is not on `PATH`, use its
absolute path in `env.nu`. `env.nu` regenerates the module at every startup, so
it follows mise upgrades.

## Xonsh {#xonsh}

Add this to `~/.xonshrc`, or to the Xonsh config file you use:

```xonsh
execx($(mise activate xonsh))
```

Restart Xonsh after saving. mise sets variables such as `$PATH` in Xonsh's
environment, which Xonsh passes to the commands it runs. It does not change
Python's `os.environ`.

## Elvish {#elvish}

Add this to `rc.elv`:

```text
var mise: = (ns [&])
eval (mise activate elvish | slurp) &ns=$mise: &on-end={|ns| set mise: = $ns }
mise:activate
```

`mise shell` and `mise deactivate` change the running shell, so they need the
module's `mise` function rather than the `mise` executable. Alias `mise` to it:

```text
edit:add-var mise~ {|@args| mise:mise $@args }
```

## Windows shells {#windows}

PowerShell uses the [PowerShell](#powershell) line above.

Native Windows mise can also activate Bash, Zsh and Fish inside Git Bash, MSYS2
and Cygwin: add the line for that shell to its startup file. mise detects the
runtime when the line runs and writes `PATH` in the form that runtime expects,
so do not generate the script in PowerShell and source it in another shell. See
[Troubleshooting](/troubleshooting.html#cygwin) for path conversion in these
shells.

cmd.exe cannot be activated. Put the shims directory on your user `Path`
instead, then open a new window:

```powershell
$shims = "$env:LOCALAPPDATA\mise\shims"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $shims) {
  [Environment]::SetEnvironmentVariable('Path', "$shims;$userPath", 'User')
}
```

Shims do not set `[env]` variables in the shell itself; see
[Shims vs PATH](/dev-tools/shims.html#shims-vs-path).

## Other shells {#other-shells}

To request support for another shell, open an
[idea discussion](https://github.com/jdx/mise/discussions/categories/ideas).
Each integration is a small module in
[`src/shell/`](https://github.com/jdx/mise/tree/main/src/shell) if you want to
contribute one.

## Shell feature support {#shell-feature-compatibility}

| Feature                              | Bash | Zsh | Fish | Nushell            | Elvish | Xonsh | PowerShell  |
| ------------------------------------ | ---- | --- | ---- | ------------------ | ------ | ----- | ----------- |
| `mise activate`                      | Yes  | Yes | Yes  | Yes                | Yes    | Yes   | Yes         |
| `mise shell` and `mise deactivate`   | Yes  | Yes | Yes  | Yes                | Yes    | Yes   | Yes         |
| Update on directory change           | Yes  | Yes | Yes  | At the next prompt | Yes    | Yes   | 7 and newer |
| [Shell aliases](/shell-aliases.html) | Yes  | Yes | Yes  | No                 | No     | No    | No          |
| `mise completion`                    | Yes  | Yes | Yes  | No                 | No     | No    | Yes         |

Elvish needs the `mise` alias from [its section](#elvish) for `mise shell` and
`mise deactivate`. PowerShell 5.1 and Nushell update the environment at the next
prompt, not on the directory change, so `cd dir; node -v` on one line still uses
the previous directory's tools. See
[Directory changes](/dev-tools/shims.html#hook-on-cd) for how each shell does
it.

## Completions {#autocompletion}

[`mise completion`](/cli/completion.html) generates completion scripts that
complete mise's commands, flags, tools, versions and task names. The scripts are
self-contained and do not need the separate `usage` CLI.

The Homebrew, Arch Linux (pacman), Ubuntu PPA and COPR (dnf) packages install
bash, zsh and fish completions for you. The extrepo apt repository, the yum and
zypper repository, and the mise.run installer do not.

`--install` writes the script where your shell looks for it. It does not edit
your startup files; for zsh and PowerShell it prints a line to add once:

::: code-group

```sh [bash]
mise completion bash --install
# writes ~/.local/share/bash-completion/completions/mise
```

```sh [zsh]
mise completion zsh --install
# writes ~/.local/share/zsh/site-functions/_mise and prints the fpath line to add
```

```sh [fish]
mise completion fish --install
# writes ~/.config/fish/completions/mise.fish
```

```powershell [PowerShell]
mise completion powershell --install
# writes ~/.config/powershell/completions/mise.ps1
# (on Windows, %LOCALAPPDATA%\PowerShell\completions\mise.ps1)
# and prints the line to add to $PROFILE
```

:::

Bash loads the script through the bash-completion package, which must be
installed and enabled in your shell.

To choose the location yourself, print the script with
`mise completion <shell>` and save it. For zsh, for example:

```sh
mkdir -p ~/.zfunc
mise completion zsh > ~/.zfunc/_mise
```

Then add these lines to `~/.zshrc`, before any existing `compinit` call
(including one a framework makes). Leave out the second line if your
`~/.zshrc` already runs `compinit`:

```zsh [~/.zshrc]
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

These scripts complete mise itself. For commands installed through the packslip
backend, see [tool completions and skills](/dev-tools/packslip-resources.html).

## Verify the setup {#verify}

Open a new terminal and run [`mise doctor`](/cli/doctor.html):

```sh
mise doctor
```

It reports `activated: yes` when the hook is loaded. If it reports
`activated: no`, check that the line is in the startup file this shell reads
and that `mise` was on `PATH` when it ran. See
[Troubleshooting](/troubleshooting.html) for other problems.
