# Comparison to asdf

mise can be used as a drop-in replacement for asdf. It supports the same `.tool-versions` files that
you may have used with asdf and uses asdf plugins. It will not, however, reuse existing asdf directories
(so you'll need to either reinstall them or move them), and 100% compatibility is not a design goal.

Casual users coming from asdf have generally found mise to just be a faster, easier to use asdf.

:::tip
Make sure you have a look at [environments](/environments.html) and [tasks](/tasks/) which
are major portions of mise that have no asdf equivalent.
:::


## Migrate from asdf to mise

If you're moving from asdf to mise, please review [#how-do-i-migrate-from-asdf](/faq.html#how-do-i-migrate-from-asdf) for guidance.

## UX

![CleanShot 2024-01-28 at 12 36 20@2x](https://github.com/jdx/mise-docs/assets/216188/47f381d7-1566-4b78-9260-3b85a21dd6ec)

Some commands are the same in asdf but others have been changed. Everything that's possible
in asdf should be possible in mise but may use slightly different syntax. mise has more forgiving commands,
such as using fuzzy-matching, e.g.: `mise install node@20`. While in asdf you _can_ run
`asdf install node latest:20`, you can't use `latest:20` in a `.tool-versions` file or many other places.
In `mise` you can use fuzzy-matching everywhere.

asdf requires several steps to install a new runtime if the plugin isn't installed, e.g.:

```sh
asdf plugin add node
asdf install node latest:20
asdf local node latest:20
```

In `mise` this can all be done in a single step which installs the plugin, installs the runtime,
and sets the version:

```sh
mise use node@20
```

If you have an existing `.tool-versions` file, or `.mise-toml`, you can install all plugins
and runtimes with a single command:

```sh
mise install
```

I've found asdf to be particularly rigid and difficult to learn. It also made strange decisions like
having `asdf list all` but `asdf latest --all` (why is one a flag and one a positional argument?).
`mise` makes heavy use of aliases so you don't need to remember if it's `mise plugin add node` or
`mise plugin install node`. If I can guess what you meant, then I'll try to get mise to respond
in the right way.

That said, there are a lot of great things about asdf. It's the best multi-runtime manager out there
and I've really been impressed with the plugin system. Most of the design decisions the authors made
were very good. I really just have 2 complaints: the shims and the fact it's written in Bash.

## Performance

asdf made (what I consider) a poor design decision to use shims that go between a call to a runtime
and the runtime itself. e.g.: when you call `node` it will call an asdf shim file `~/.asdf/shims/node`,
which then calls `asdf exec`, which then calls the correct version of node.

These shims have terrible performance, adding ~120ms to every runtime call. `mise activate` does not use shims and instead
updates `PATH` so that it doesn't have any overhead when simply calling binaries. These shims are the main reason that I wrote this. Note that in the demo GIF at the top of this README
that `mise` isn't actually used when calling `node -v` for this reason. The performance is
identical to running node without using mise.

I don't think it's possible for asdf to fix these issues. The author of asdf did a great writeup
of [performance problems](https://stratus3d.com/blog/2022/08/11/asdf-performance/). asdf is written
in bash which certainly makes it challenging to be performant, however I think the real problem is the
shim design. I don't think it's possible to fix that without a complete rewrite.

mise does call an internal command `mise hook-env` every time the directory has changed, but because
it's written in Rust, this is very quick—taking ~10ms on my machine. 4ms if there are no changes, 14ms if it's
a full reload.

tl;dr: asdf adds overhead (~120ms) when calling a runtime, mise adds a small amount of overhead (~5ms)
when the prompt loads.

## Command Compatibility

In nearly all places you can use the exact syntax that works in asdf, however this likely won't
show up in the help or CLI reference. If you're coming from asdf and comfortable with that way of
working you can almost always use the same syntax with mise, e.g.:

```sh
mise install node 20.0.0
mise local node 20.0.0
```

It's not recommended though. You almost always want to modify config files and install things so
`mise use node@20` saves an extra command. Also, the "@" in the command is preferred since it allows
you to install multiple tools at once: `mise use|install node@20 node@18`. Also, there are edge cases
where it's not possible—or at least very challenging—for us to definitively know which syntax is being
used and so we default to mise-style. While there aren't many of these, asdf-compatibility is done
as a "best-effort" in order to make transitioning from asdf feel familiar for those users who can
rely on their muscle memory. Ensuring asdf-syntax works with everything is not a design goal.

## Extra backends <Badge type="warning" text="experimental" />

mise has support for backends other than asdf plugins. For example you can install CLIs
directly from cargo and npm:

```sh
mise use -g cargo:ripgrep@14
mise use -g npm:prettier@3
```
