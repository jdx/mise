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

Before submitting a ticket, it's a good idea to test what you were doing with asdf. That way we can
rule
out if the issue is with mise or if it's with a particular plugin. For example,
if `mise install python@latest`
doesn't work, try running `asdf install python latest` to see if it's an issue with asdf-python.

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

The second uses the mise-versions.jdx.dev host as a centralized
place to list all of the versions of most plugins. This is intended to speed up mise and also
get around GitHub rate limits when querying for new versions. Check that repo for your plugin to
see if it has an updated version. This service can be disabled by
setting `MISE_USE_VERSIONS_HOST=0`.

## Windows problems

Very basic support for windows is currently available, however because Windows can't support asdf
plugins, they must use core and vfox only—which means only a handful of tools are available on
Windows.

As of this writing, env var management and task execution are not yet supported on Windows.

## mise isn't working when calling from tmux or another shell initialization script

`mise activate` will not update PATH until the shell prompt is displayed. So if you need to access a
tool provided by mise before the prompt is displayed you can either
[add the shims to your PATH](getting-started.html#2-add-mise-shims-to-path) e.g.

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
