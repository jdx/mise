# asdf Backend

asdf is the original backend for mise. It's the default if no backend is specified,
e.g.: `mise i ripgrep` will use [asdf](https://gitlab.com/wt0f/asdf-ripgrep) but `mise i cargo:ripgrep`
will use the [cargo](./cargo) backend. You can explicitly specify the asdf backend with `mise i asdf:ripgrep`.
If you wish.

There are [hundreds of plugins](https://github.com/mise-plugins/registry) available in the
[mise registry](https://github.com/mise-plugins) and you can also install plugins from git
repos or local directories.

::: warning
Take care using plugins as well as anything else you get from the internet. CLIs are
unfortunately capable of doing a lot of damage to your system if a bad actor manages to
get into your system through a plugin or other tool.
:::

## Writing asdf plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).

_TODO: document special features only available in mise._
