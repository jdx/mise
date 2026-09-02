# direnv <Badge type="warning" text="deprecated" />

[direnv](https://direnv.net) and mise both manage environment variables based on directory. Because
they both analyze
the current environment variables before and after their respective "hook" commands are run, they
can sometimes conflict with each other.

::: warning
The official stance is you should not use direnv with mise. Issues arising
from incompatibilities are not considered bugs and PRs to improve direnv
compatibility will not be accepted.
While that's the official stance, the reality is mise and direnv can
coexist for simple cases like setting unrelated environment variables.
Anything involving PATH — which is most of what people use both tools
for — is where problems arise.
:::

If you have an issue, it's likely to do with the ordering of PATH. This is really only a problem
if you are trying to manage the same tool with both direnv and mise. For example, you may use
`layout python` in an `.envrc` while also maintaining a `.tool-versions` file with python in it.

A more typical use of direnv is to set arbitrary environment variables or add unrelated
binaries to PATH. In these cases, mise does not interfere with direnv.

## mise inside of direnv (`use mise` in `.envrc`)

::: warning
`use mise` is deprecated and no longer supported.
:::

If you encounter issues with `mise activate`, or want to use direnv in a different way,
this is a simpler setup that's less likely to cause issues—at the cost of functionality.

This may be required if you want to use direnv's `layout python` with mise. Otherwise, there are
situations where mise overrides direnv's PATH. `use mise` ensures that direnv always has
control.

To do this, first use `mise` to build a `use_mise` function that you can use in `.envrc` files:

```sh
mise direnv activate > ~/.config/direnv/lib/use_mise.sh
```

Now add the following to your `.envrc` file:

```sh
use mise
```

direnv now calls mise to export its environment variables. Make sure to add `use_mise`
to all projects that use mise (or use direnv's `source_up` to load it from a subdirectory). You can
also add `use mise` to `~/.config/direnv/direnvrc`.

With this method, direnv typically won't know to refresh `.tool-versions` files
unless they're at the same level as an `.envrc` file, so you'll likely always want
an `.envrc` file next to your `.tool-versions`. To make this easier to manage,
I encourage _not_ using `.tool-versions` at all, and instead
setting environment variables entirely in `.envrc`:

```sh
export MISE_NODE_VERSION=20.0.0
export MISE_PYTHON_VERSION=3.11
```

Of course, if you use `mise activate`, these steps aren't necessary and you can use
mise as if direnv were not in use.

If you continue to struggle, you can also try using the [shims method](dev-tools/shims.md).

### Do you need direnv?

mise can replace direnv for most use cases. This is why mise includes support for
managing env vars and [virtualenv](lang/python.md#automatic-virtualenv-activation)
for python using `mise.toml`.
