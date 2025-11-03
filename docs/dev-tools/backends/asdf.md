# asdf Backend

`asdf` is the original backend for mise.

It relies on asdf plugins for each tool. asdf plugins are more risky to use because they're typically written by a single developer unrelated to the tool vendor. They also generally do not function on Windows because they're written
in bash which is often not available on Windows and the scripts generally are not written to be cross-platform.

asdf plugins are not used for tools inside the [registry](https://github.com/jdx/mise/blob/main/registry.toml) whenever possible. Sometimes it is not possible to use more secure backends like aqua/ubi because tools have complex install setups or need to export env vars.

All of these are hosted in the mise-plugins org to secure the supply chain so you do not need to rely on plugins maintained by anyone except me.

Because of the extra complexity of asdf tools and security concerns we are actively moving tools in
the registry away from asdf where possible to backends like aqua and ubi which don't require plugins.
That said, not all tools can function with ubi/aqua if they have a unique installation process or
need to set env vars other than `PATH`.

## Writing asdf (legacy) plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).
