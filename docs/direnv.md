# direnv <Badge type="warning" text="deprecated" />

[direnv](https://direnv.net) and mise both change the environment when you enter a
directory. Their shell hooks can disagree about which `PATH` entries to add,
restore, or remove.

::: warning Unsupported integration
Using direnv with mise is unsupported. Compatibility issues are not considered
mise bugs, and PRs for direnv compatibility are not accepted. The `use mise`
integration is deprecated.
:::

## Do you need direnv? {#do-you-need-direnv}

For a project that uses direnv to set variables, load dotenv files, or activate a
Python environment, mise has corresponding configuration:

| Existing `.envrc` behavior       | mise configuration                                                                   |
| -------------------------------- | ------------------------------------------------------------------------------------ |
| `export NODE_ENV=development`    | `[env]` with `NODE_ENV = "development"`                                              |
| Load a dotenv file               | [`env._.file`](/environments/#env-file)                                              |
| Add `bin` to `PATH`              | [`env._.path`](/environments/#env-path)                                              |
| Export values from a Bash script | [`env._.source`](/environments/#env-source)                                          |
| Activate a Python virtualenv     | [Python virtualenv configuration](/lang/python.html#automatic-virtualenv-activation) |

For example:

```toml [mise.toml]
[env]
NODE_ENV = "development"
_.file = ".env"
_.path = "bin"
```

This example assumes the project has a `.env` file. Remove that directive if it
does not. See [Environments](/environments/) for defaults, unsetting values, and
sourcing scripts.

After moving the required behavior into `mise.toml`, remove the project's direnv
integration, [activate mise](/getting-started.html#activate-mise), and open a fresh
shell to verify the environment. `mise exec -- <command>` can check project
commands without depending on the interactive shell's current state.

## mise inside of direnv (`use mise` in `.envrc`)

The following describes the deprecated setup for people maintaining or removing
an existing integration. It gives direnv control of the exported environment and
does not provide mise's full activation behavior.

The integration generates a direnv library function:

```sh
mkdir -p ~/.config/direnv/lib
mise direnv activate > ~/.config/direnv/lib/use_mise.sh
```

An `.envrc` then calls it as:

```sh
use mise
```

Keep the distinction between the shell function `use_mise` and direnv's
`use mise` syntax. Existing projects may also load it from a parent `.envrc` with
`source_up`, or from `~/.config/direnv/direnvrc`.

If retaining this integration, avoid having both tools manage the same runtime
or virtualenv. A common conflict is direnv's `layout python` alongside a Python
version selected by mise. Changes to a `.tool-versions` file outside the `.envrc`
directory may also fail to trigger a direnv refresh.

[Shims](/dev-tools/shims.html) provide another way to run mise-managed tools, but
they do not reproduce all the features of `mise activate` or make mixed shell
hooks a supported setup.
