# Paranoid

Paranoid is an optional behavior that locks mise down more to make it harder
for a bad actor to compromise your system. These are settings that I 
personally do not use on my own system because I find the behavior too
restrictive for the benefits.

Paranoid mode can be enabled with either `MISE_PARANOID=1` or a setting:

```sh
$ mise settings set paranoid 1
```

## Config files

Normally `mise` will make sure some config files are "trusted" before loading
them. This will prompt you to confirm that you want to load the file, e.g.:

```sh
$ mise install
mise ~/src/mise/.tool-versions is not trusted. Trust it [y/n]?
```

Generally only potentially dangerous config files are checked such as files
that use templates (which can execute arbitrary code) or that set env vars.
Under paranoid, however, all config files must be trusted first.

Also, in normal mode, a config file only needs to be trusted a single time.
In paranoid, the contents of the file are hashed to check if the file changes.
If you change your config file, you'll need to trust it again.

## Community plugins

Community plugins can not be directly installed via short-name under paranoid.
You can install plugins that are either core, maintained by the mise team,
or plugins that mise has marked as "first-party"—meaning plugins developed by
the same team that builds the tool the plugin installs.

Other than that, say for "shfmt", you'll need to specify the full git repo
to install:

```sh
$ mise plugin install shfmt https://github.com/luizm/asdf-shfmt
```

Unlike in normal mode where `mise plugin install shfmt` would be sufficient.

## More?

If you have suggestions for more that could be added to paranoid, please let
me know.
