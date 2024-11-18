# asdf Backend

asdf is the original backend for mise. It's the default if no backend is specified,
e.g.: `mise i ripgrep` will use [asdf](https://gitlab.com/wt0f/asdf-ripgrep) but `mise i cargo:ripgrep`
will use the [cargo](./cargo) backend. You can explicitly specify the asdf backend with `mise i asdf:ripgrep`.
If you wish.

If choosing a backend to integrate a tool into mise, it's discouraged to use the asdf backend. ubi
would be the ideal choice if it can work as a single binary, otherwise aqua would be the next best choice
since it requires minimal configuration and doesn't require executing code in a plugin. Generally
vfox plugins can handle anything an asdf plugin might need to do while also being potentially able
to support windows.

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
