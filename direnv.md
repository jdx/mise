---
---

# direnv

[direnv](https://direnv.net) and mise both manage environment variables based on directory. Because they both analyze
the current environment variables before and after their respective "hook" commands are run, they can sometimes conflict with each other.

If you have an issue, it's likely to do with the ordering of PATH. This means it would
really only be a problem if you were trying to manage the same tool with direnv and mise. For example,
you may use `layout python` in an `.envrc` but also be maintaining a `.tool-versions` file with python
in it as well.

A more typical usage of direnv would be to set some arbitrary environment variables, or add unrelated
binaries to PATH. In these cases, mise will not interfere with direnv.

### mise inside of direnv (`use mise` in `.envrc`)

If you do encounter issues with `mise activate`, or just want to use direnv in an alternate way,
this is a simpler setup that's less likely to cause issues—at the cost of functionality.

This may be required if you want to use direnv's `layout python` with mise. Otherwise there are
situations where mise will override direnv's PATH. `use mise` ensures that direnv always has control.

To do this, first use `mise` to build a `use_mise` function that you can use in `.envrc` files:

```sh
mise direnv activate > ~/.config/direnv/lib/use_mise.sh
```

Now in your `.envrc` file add the following:

```sh
use mise
```

direnv will now call mise to export its environment variables. You'll need to make sure to add `use_mise`
to all projects that use mise (or use direnv's `source_up` to load it from a subdirectory). You can also add `use mise` to `~/.config/direnv/direnvrc`.

Note that in this method direnv typically won't know to refresh `.tool-versions` files
unless they're at the same level as a `.envrc` file. You'll likely always want to have
a `.envrc` file next to your `.tool-versions` for this reason. To make this a little
easier to manage, I encourage _not_ actually using `.tool-versions` at all, and instead
setting environment variables entirely in `.envrc`:

```sh
export MISE_NODE_VERSION=20.0.0
export MISE_PYTHON_VERSION=3.11
```

Of course if you use `mise activate`, then these steps won't have been necessary and you can use mise
as if direnv was not used.

If you continue to struggle, you can also try using the [shims method](./shims).

### Do you need direnv?

While making mise compatible with direnv is, and will always be a major goal of this project, I also
want mise to be capable of replacing direnv if needed. This is why mise includes support for managing
env vars and [virtualenv](https://github.com/jdx/mise/blob/main/docs/python.md#experimental-automatic-virtualenv-creationactivation)
for python using `.mise.toml`.

If you find you continue to need direnv, please open an issue and let me know what it is to see if
it's something mise could support. mise will never be as capable as direnv with a DSL like `.envrc`,
but I think we can handle enough common use cases to make that unnecessary for most people.
