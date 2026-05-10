# Troubleshooting

## `mise activate` doesn't work in `~/.profile`, `~/.bash_profile`, `~/.zprofile`

`mise activate` should only be used in `rc` files. These are the interactive ones used when
a real user is using the terminal. (As opposed to being executed by an IDE or something). The prompt
isn't displayed in non-interactive environments so PATH won't be modified.

For non-interactive setups, consider using shims instead which will route calls to the correct
directory by looking at `PWD` every time they're executed. You can also call `mise exec` instead of
expecting things to be directly on PATH. You can also run `mise env` in a non-interactive shell,
however that
will only setup the global tools. It won't modify the environment variables when entering into a
different project.

::: warning
`mise activate --shims` does not support all the features of `mise activate`.<br>
See [shims vs path](/dev-tools/shims.html#shims-vs-path) for more info.
:::

Also see the [shebang](/tips-and-tricks#shebang) example for a way to make scripts call mise to get
the runtime.
That is another way to use mise without activation.

## Slow shell prompts {#slow-shell-prompts}

`mise activate` runs a hook on every prompt to check if tools or env vars need updating. This typically takes only a few milliseconds, but if your prompts feel sluggish you can profile it with `MISE_TIMINGS`:

First deactivate mise so the prompt hook doesn't interfere with your measurement, then run `hook-env` manually with timings:

```sh
mise deactivate

# Show timing per major step (color-coded: red = slow)
MISE_TIMINGS=1 mise hook-env -s bash 2>&1 >/dev/null

# Or use =2 for detailed per-step breakdowns with cumulative time
MISE_TIMINGS=2 mise hook-env -s bash 2>&1 >/dev/null
```

Replace `bash` with your shell. Common causes of slow prompts:

- Expensive `_.source` scripts in `mise.toml` — these re-run on every prompt
- Large numbers of tools or plugins
- Network-dependent operations in env directives

Note that [`mise activate --shims`](/dev-tools/shims) moves the cost from every prompt to every tool invocation, which may or may not be faster depending on your workflow. See [Shims vs PATH](/dev-tools/shims.html#shims-vs-path) for tradeoffs.

## mise is failing or not working right

First try setting `MISE_DEBUG=1` or `MISE_TRACE=1` and see if that gives you more information.
You can also set `MISE_LOG_FILE_LEVEL=debug MISE_LOG_FILE=/path/to/logfile` to write logs to a file.

If something is happening with the activate hook, you can try disabling it and
calling `eval "$(mise hook-env)"` manually.
It can also be helpful to use `mise env` which will just output environment variables that would be
set.
Also consider using [shims](/dev-tools/shims.md) which can be more compatible.

If runtime installation isn't working right, try using the `--raw` flag which will install things in
series and connect stdin/stdout/stderr directly to the terminal. If a plugin is trying to interact
with you for some reason this will make it work.

Of course check the version of mise with `mise --version` and make sure it is the latest.
Use `mise self-update`
to update it. `mise cache clean` can be used to wipe the internal cache and `mise implode` can be
used
to remove everything except config.

Lastly, there is `mise doctor` which will show diagnostic information and any warnings about issues
detected with your setup. If you submit a bug report, please include the output of `mise doctor`.

## The wrong version of a tool is being used

Likely this means that mise isn't first in PATH—using shims or `mise activate`. You can verify if
this is the case by calling `which -a`, for example, if node@20.0.0 is being used but mise specifies
node@26.0.0, first make sure that mise has this version installed and active by running `mise ls node`.
It should not say missing and have the correct "Requested" version:

```bash
$ mise ls node
Plugin  Version  Config Source       Requested
node    24.0.0  ~/.mise/config.toml  24.0.0
```

If `node -v` isn't showing the right version, make sure mise is activated by running `mise doctor`.
It should not have a "problem" listed about mise not being activated. Lastly, run `which -a node`.
If the directory listed is not a mise directory, then mise is not first in PATH. Whichever node is
being run first needs to have its directory set before mise is. Typically this means setting PATH for
mise shims at the end of bashrc/zshrc.

If using `mise activate`, you have another option of enabling `MISE_ACTIVATE_AGGRESSIVE=1` which will
have mise always prepend its tools to be first in PATH. If you're using something that also modifies
paths dynamically like `mise activate` does, this may not work because the other tool may be modifying
PATH after mise does.

If nothing else, you can run things with [`mise x --`](/cli/exec) to ensure that the correct version is being used.

## New version of a tool is not available

There are 2 places that versions are cached so a brand new release might not appear right away.

The first is that the mise CLI caches versions for. The cache can be cleared with `mise cache clear`.

The second uses the <https://mise-versions.jdx.dev> host as a centralized
place to list all of the versions of most plugins. This is intended to speed up mise and also
get around GitHub rate limits when querying for new versions. Check that repo for your plugin to
see if it has an updated version. This service can be disabled by
setting `MISE_USE_VERSIONS_HOST=0`.

mise-versions itself also struggles with rate limits but you can help it to fetch more frequently by authenticating
with its [GitHub app](https://github.com/apps/mise-versions). It does not require any permissions since it simply
fetches public repository information. The more people do this, the quicker
mise will be able to fetch new versions of tools.

## Windows problems

::: warning
Very basic support for windows is currently available, however because Windows can't support asdf
plugins, they must use core and vfox only—which means only a handful of tools are available on
Windows.
:::

### Path limits

If you have many tools defined in your `mise.toml` hierarchy, then it is possible that `mise x` will produce a `Path` environment variable that is too long for certain tools to handle, most notably, `cmd.exe`. This will affect `mise` tools that invoke `cmd.exe` (like `npm install`).

You have a few options:

1. Set the `MISE_INSTALLS_DIR` environment variable to a shorter location, e.g. `C:\.mise-installs`.
1. Use `powershell.exe` or `pwsh.exe` instead of `cmd.exe`, since they can handle a longer `Path`.
1. Re-organise the `mise.toml` files in your monorepo, to specify only the tools they need.

You can run the following command to test whether you have hit the `cmd.exe` `Path` limitation:

```powershell
# Path is within limits
❯ mise x -- cmd.exe /d /s /c "where.exe where"
C:\Windows\System32\where.exe
# Path exceeds cmd.exe limits
❯ mise x -- cmd.exe /d /s /c "where.exe where"
'where.exe' is not recognized as an internal or external command,
operable program or batch file.
mise ERROR command failed: exit code 1
mise ERROR Run with --verbose or MISE_VERBOSE=1 for more information
```

### `shell = "bash -c"` task fails with `command not found` from PowerShell

If a task pinned to `shell = "bash -c"` works from Git Bash but fails with
`command not found` from PowerShell, mise is most likely resolving `bash` to
the WSL launcher at `C:\Windows\System32\bash.exe` instead of a real POSIX
bash. The launcher dispatches into the WSL distribution's Linux user-space,
where mise-managed Windows tools aren't visible.

mise prefers a real POSIX bash (Git Bash / MSYS2) automatically when it can
find one in a standard install location. If yours is installed elsewhere, set
`MISE_BASH_PATH` to override:

```powershell
$env:MISE_BASH_PATH = "C:\tools\msys64\usr\bin\bash.exe"
mise run my-bash-task
```

```toml
# Alternatively, scope it to one project from mise.toml
[env]
MISE_BASH_PATH = "C:/tools/msys64/usr/bin/bash.exe"
```

## mise isn't working when calling from tmux or another shell initialization script

`mise activate` will not update PATH until the shell prompt is displayed. So if you need to access a
tool provided by mise before the prompt is displayed you can either
[add the shims to your PATH](/dev-tools/shims.html#how-to-add-mise-shims-to-path) e.g.

```bash
export PATH="$HOME/.local/share/mise/shims:$PATH"
python --version # will work after adding shims to PATH
```

Or you can manually call `hook-env`:

```bash
eval "$(mise activate bash)"
eval "$(mise hook-env)"
python --version # will work only after calling hook-env explicitly
```

For more information, see [What does `mise activate` do?](/faq#what-does-mise-activate-do)

## Is mise secure?

Providing a secure supply chain is incredibly important. mise already provides a more secure
experience when compared to asdf. Security-oriented evaluations and contributions are welcome.
We also urge users to look after the plugins they use, and urge plugin authors to look after
the users they serve.

For more details see [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).

## 403 Forbidden when installing a tool

You may get an error like one of the following:

```text
HTTP status client error (403 Forbidden) for url
403 API rate limit exceeded for
```

This can happen if the tool is hosted on GitHub, and you've hit the API rate limit. This is especially
common running mise in a CI environment like GitHub Actions.

See [GitHub Tokens](/dev-tools/github-tokens.html) for how to configure authentication and avoid rate limits.

## Tool not found after `mise install` or `mise use` in a script

If you run `mise use` or `mise install` inside a script and then immediately try to use the
tool, it may not be found. This is because `mise activate` updates PATH at the next prompt,
which never happens in a script.

**Solutions:**

```bash
# Option 1: Use mise exec (recommended)
mise install
mise exec -- my-tool --version

# Option 2: Re-evaluate the environment after install
mise install
eval "$(mise hook-env)"
my-tool --version

# Option 3: Use shims (they always resolve dynamically)
export PATH="$HOME/.local/share/mise/shims:$PATH"
mise install
my-tool --version
```

## Creating `~/.bash_profile` breaks existing `~/.profile` on Ubuntu/Debian

On many Linux distributions, `~/.profile` sources `~/.bashrc` and sets up your environment.
However, if `~/.bash_profile` exists, bash reads that **instead of** `~/.profile`.

If you followed setup instructions that created `~/.bash_profile` for mise, your existing
`~/.profile` configuration (including PATH, environment variables, etc.) may stop loading.

**Fix:** Add mise activation to `~/.bashrc` instead, or source `~/.profile` from your
`~/.bash_profile`:

```bash
# ~/.bash_profile
[[ -f ~/.profile ]] && source ~/.profile
```

## Tasks with `redact` env vars break `raw` output

If you have `redact = true` on any env var in your config, tasks with `raw = true` will appear
to produce no output. This is because mise intercepts stdout/stderr to perform redaction, which
conflicts with raw mode.

**Workaround**: Remove `redact` from env vars that don't need it, or accept that raw tasks
won't produce visible output when redactions are active.

## `mise activate` in CI / non-interactive shells

`mise activate` hooks into the shell prompt to update PATH, so historically it didn't work
in non-interactive shells. With the addition of `chpwd` support, it does work in more
situations now, but we still recommend these approaches for CI and scripts:

```bash
# Option 1: Use shims (recommended for CI)
export PATH="$HOME/.local/share/mise/shims:$PATH"
# In GitHub Actions, use: echo "$HOME/.local/share/mise/shims" >> $GITHUB_PATH

# Option 2: Use mise exec
mise exec -- npm test

# Option 3: Manually call hook-env after activate
eval "$(mise activate bash)"
eval "$(mise hook-env)"
```

See also the [CI/CD section](/tips-and-tricks.html#ci-cd) in Tips & Tricks.

## Auto-install on command not found handler does not work for new tools

If you are expecting mise to automatically install a tool when you run a command that is not found (using the [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) feature), be aware of an important limitation:

**mise can only auto-install missing versions of tools that already have at least one version installed.**

This is because mise does not have a way of knowing which binaries a tool provides unless there is already an installed (even inactive) version of that tool. If you have never installed any version of a tool, mise cannot determine which tool is responsible for a given binary name, and so it cannot auto-install it on demand.

**Workarounds:**

- Manually install at least one version of the tool you want to be auto-installed in the future. After that, the auto-install feature will work for missing versions of that tool.
- Use [`mise x|exec`](/cli/exec) or [`mise r|run`](/cli/run) to trigger auto-install for missing tools, even if no version is currently installed. These commands will attempt to install the required tool versions automatically.
