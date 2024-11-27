# asdf Backend

asdf is the original backend for mise. It relies on asdf plugins for each tool. asdf plugins are
more risky to use because they're typically written by a single developer unrelated to the tool vendor
they also do not function on Windows.
Because of the extra complexity of asdf tools and security concerns we are actively moving tools in
the registry away from asdf where possible to backends like aqua and ubi which don't require plugins.
That said, not all tools can function with ubi/aqua if they have a unique installation process or
need to set env vars other than PATH.

## Writing asdf plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).

_TODO: document special features only available in mise._
