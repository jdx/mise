# Comparison to asdf

mise can be used as a drop-in replacement for asdf. It supports the same `.tool-versions` files that
you may have used with asdf and can use asdf plugins through
the [asdf backend](/dev-tools/backends/asdf.html).

It will not, however, reuse existing asdf directories
(so you'll need to either reinstall them or move them), and 100% compatibility is not a design goal.
That said,
if you're coming from asdf-bash (0.15 and below), mise actually
has [fewer breaking changes than asdf-go (0.16 and above)](https://asdf-vm.com/guide/upgrading-to-v0-16.html)
despite 100% compatibility not being a design goal of mise.

Casual users coming from asdf have generally found mise to just be a faster, easier to use asdf.

:::tip
Make sure you have a look at [environments](/environments/) and [tasks](/tasks/) which
are major portions of mise that have no asdf equivalent.
:::

## Migrate from asdf to mise

If you're moving from asdf to mise, please
review [#how-do-i-migrate-from-asdf](/faq.html#how-do-i-migrate-from-asdf) for guidance.

## asdf in go (0.16+)

asdf has gone through a rewrite in go. Because this is quite new as of this writing (2025-01-01),
I'm going to keep information about 0.16+ asdf versions (which I call "asdf-go" vs "asdf-bash") in
this section and the rest of this doc will apply to asdf-bash (0.15 and below).

In terms of performance, mise is still faster than the go asdf, however the difference is much
closer. asdf is likely fast enough that the difference in overhead between asdf-go and mise may not
even be enough to notice for you—after all there are plenty of people still using asdf-bash that
claim they don't even notice how slow it is (don't ask me how):

![GgAQJJmWIAAUlec](https://github.com/user-attachments/assets/05689925-396d-41f3-bcd1-7b3b1bf6c2fa)

I don't think performance is a good enough reason to switch though now that asdf-go is a thing. It's
a reason, but it's a minor one. The improved security in mise, better DX, and lack of reliance on
shims are all more important than performance.

Given they went through the trouble of rewriting asdf—that's also an indication they want to keep
working on it (which is awesome that they're doing that btw). This does mean that some of what's
written here may go out of date if they address some of the problems
with asdf.

## Supply chain security

asdf plugins are not secure. This is explained
in [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md), but the quick explanation is
that asdf plugins involve shell code which can essentially do anything on your machine. It's
dangerous code. What's worse is asdf plugins are rarely written by the tool vendor (who you need to
trust anyway to use the tool), which means for every asdf plugin you use you'll be trusting a random
developer to not go rogue and to not get hacked themselves and publish changes to a plugin with an
exploit.

mise still uses asdf plugins for some tools, but we're actively reducing that count as well as
moving things into the [mise-plugins org](https://github.com/mise-plugins). It looks like asdf has a
similar model with their asdf-community org, but it isn't. asdf gives plugin authors commit access
to their plugin in [asdf-community](https://github.com/asdf-community) when they move it in, which I
feel like defeats the purpose of having a dedicated org in the first place. By the end of 2025 I
would like for there to no longer be any asdf plugins in the registry that aren't owned by me.

I've also been adopting extra security verification steps when vendors offer that ability such as
gpg verification on node installs, and native Cosign/SLSA/Minisign/GitHub attestation verification for aqua tools.

## UX

![CleanShot 2024-01-28 at 12 36 20@2x](https://github.com/jdx/mise-docs/assets/216188/47f381d7-1566-4b78-9260-3b85a21dd6ec)

Some commands are the same in asdf but others have been changed. Everything that's possible
in asdf should be possible in mise but may use slightly different syntax. mise has more forgiving
commands,
such as using fuzzy-matching, e.g.: `mise install node@20`. While in asdf you _can_ run
`asdf install node latest:20`, you can't use `latest:20` in a `.tool-versions` file or many other
places.
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
and the runtime itself. e.g.: when you call `node` it will call an asdf shim file
`~/.asdf/shims/node`,
which then calls `asdf exec`, which then calls the correct version of node.

These shims have terrible performance, adding ~120ms to every runtime call. `mise activate` does not
use shims and instead
updates `PATH` so that it doesn't have any overhead when simply calling binaries. These shims are
the main reason that I wrote this. Note that in the demo GIF at the top of this README
that `mise` isn't actually used when calling `node -v` for this reason. The performance is
identical to running node without using mise.

I don't think it's possible for asdf to fix these issues. The author of asdf did a great writeup
of [performance problems](https://stratus3d.com/blog/2022/08/11/asdf-performance/). asdf is written
in bash which certainly makes it challenging to be performant, however I think the real problem is
the
shim design. I don't think it's possible to fix that without a complete rewrite.

mise does call an internal command `mise hook-env` every time the directory has changed, but because
it's written in Rust, this is very quick—taking ~10ms on my machine. 4ms if there are no changes,
14ms if it's
a full reload.

tl;dr: asdf adds overhead (~120ms) when calling a runtime, mise adds a small amount of overhead (~
5ms)
when the prompt loads.

## Windows support

asdf does not run on Windows at all. With mise, tools using non-asdf backends can support Windows.
Of course, this means the tool
vendor must provide Windows binaries but if they do, and the backend isn't asdf, the tool should
work on Windows.

## Security

asdf plugins are insecure. They typically are written by individuals with no ties to the vendors
that provide the underlying tool.
Where possible, mise does not use asdf plugins and instead uses backends like aqua and ubi which do
not require separate plugins.

Aqua tools include native Cosign/SLSA/Minisign/GitHub attestation verification built into mise.
See [SECURITY](https://github.com/jdx/mise/blob/main/SECURITY.md) for more information.

## Command Compatibility

In nearly all places you can use the exact syntax that works in asdf, however this likely won't
show up in the help or CLI reference. If you're coming from asdf and comfortable with that way of
working you can almost always use the same syntax with mise, e.g.:

```sh
mise install node 20.0.0
mise local node 20.0.0
```

UPDATE (2025-01-01): asdf-go (0.16+) actually got rid of `asdf global|local` entirely in favor of
`asdf set` which we can't support since we already have a command named `mise set`. mise command
compatibility will likely not be as good with asdf-go 0.16+.

It's not recommended though. You almost always want to modify config files and install things so
`mise use node@20` saves an extra command. Also, the "@" in the command is preferred since it allows
you to install multiple tools at once: `mise use|install node@20 node@18`. Also, there are edge
cases
where it's not possible—or at least very challenging—for us to definitively know which syntax is
being
used and so we default to mise-style. While there aren't many of these, asdf-compatibility is done
as a "best-effort" in order to make transitioning from asdf feel familiar for those users who can
rely on their muscle memory. Ensuring asdf-syntax works with everything is not a design goal.

## Extra backends

mise has support for backends other than asdf plugins. For example you can install CLIs
directly from cargo and npm:

```sh
mise use -g cargo:ripgrep@14
mise use -g npm:prettier@3
```
