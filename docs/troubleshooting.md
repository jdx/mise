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
node@22.0.0, first make sure that mise has this version installed and active by running `mise ls node`.
It should not say missing and have the correct "Requested" version:

```bash
$ mise ls node
Plugin  Version  Config Source       Requested
node    22.0.0  ~/.mise/config.toml  22.0.0
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
common running mise in a CI environment like GitHub Actions. If you don't have a `GITHUB_TOKEN`
set, the rate limit is quite low. You can fix this by creating a GitHub token (which needs no scopes)
by going to [https://github.com/settings/tokens/new](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) and setting it as an environment variable. You can
use any of the following (in order of preference):

- `MISE_GITHUB_TOKEN`
- `GITHUB_TOKEN`
- `GITHUB_API_TOKEN`

## Auto-install on command not found handler does not work for new tools

If you are expecting mise to automatically install a tool when you run a command that is not found (using the [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) feature), be aware of an important limitation:

**mise can only auto-install missing versions of tools that already have at least one version installed.**

This is because mise does not have a way of knowing which binaries a tool provides unless there is already an installed (even inactive) version of that tool. If you have never installed any version of a tool, mise cannot determine which tool is responsible for a given binary name, and so it cannot auto-install it on demand.

**Workarounds:**

- Manually install at least one version of the tool you want to be auto-installed in the future. After that, the auto-install feature will work for missing versions of that tool.
- Use [`mise x|exec`](/cli/exec) or [`mise r|run`](/cli/run) to trigger auto-install for missing tools, even if no version is currently installed. These commands will attempt to install the required tool versions automatically.
