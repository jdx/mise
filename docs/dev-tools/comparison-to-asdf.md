# Comparison to asdf

mise can be used as a drop-in replacement for asdf. It supports the same `.tool-versions` files that
you may have used with asdf and can use asdf plugins through the [asdf backend](/dev-tools/backends/asdf.html).

It will not, however, reuse existing asdf directories
(so you'll need to either reinstall them or move them), and 100% compatibility is not a design goal. That said,
if you're coming from asdf-bash (0.15 and below), mise actually has [fewer breaking changes than asdf-go (0.16 and above)](https://asdf-vm.com/guide/upgrading-from-v0-15-to-v0-16.html) despite 100% compatibility not being a design goal of mise.

Casual users coming from asdf have generally found mise to just be a faster, easier to use asdf.

:::tip
Make sure you have a look at [environments](/environments/) and [tasks](/tasks/) which
are major portions of mise that have no asdf equivalent.
:::

## Migrate from asdf to mise

If you're moving from asdf to mise, please review [#how-do-i-migrate-from-asdf](/faq.html#how-do-i-migrate-from-asdf) for guidance.

## asdf in go (0.16+)

asdf has gone through a rewrite in go. Because this is quite new as of this writing (2025-01-01), I'm going to keep information about 0.16+ asdf versions (which I call "asdf-go" vs "asdf-bash") in this section and the rest of this doc will apply to asdf-bash (0.15 and below).

In terms of performance, mise is still faster than the go asdf, however the difference is much closer. asdf is likely fast enough that the difference in overhead between asdf-go and mise may not even be enough to notice for you—after all there are plenty of people still using asdf-bash that claim they don't even notice how slow it is (don't ask me how):

![GgAQJJmWIAAUlec](https://github.com/user-attachments/assets/05689925-396d-41f3-bcd1-7b3b1bf6c2fa)

I don't think performance is a good enough reason to switch though now that asdf-go is a thing. It's a reason, but it's a minor one. The improved security in mise, better DX, and lack of reliance on shims.

Given they went through the trouble of rewriting asdf—that's also an indication they want to keep working on it (which is awesome that they're doing that btw). This does mean that some of what's written here may go out of date if they address some of the problems
with asdf.

## Supply chain security

asdf plugins are not very secure. This is explained on the [asdf backend page](https://mise.jdx.dev/dev-tools/backends/asdf.html), but the quick explanation is that asdf plugins involve shell code which can essentially do anything on your machine. It's dangerous code. What's worse is asdf plugins are rarely written by the tool vendor (who you need to trust anyway to use the tool), which means for every asdf plugin you use you'll be trusting a random developer to not go rogue and to not get hacked themselves and publish changes to a plugin with an exploit.

While mise still uses asdf plugins for some tools, the count is (as of this writing, 2025-01-01) ~35% of tools in the default mise registry use asdf as the primary backend. aqua/ubi are the preferred backends, however not all tools can work with aqua or ubi—it's basically just tools that have GitHub Releases of precompiled binaries that can. If something needs to be compiled or uses features like custom env vars, it needs to use an asdf plugin. (vfox can also be used, but vfox suffers from the same supply-chain issue as asdf).

We've been working on reducing this number and hopefully by the end of 2025 we will have either moved everything over to safer backends or forked/moved all the asdf plugins into the [mise-plugins org](https://github.com/mise-plugins). The mise-plugins org also solves this issue for us
since that org is owned by me—a developer going rogue would need to submit a PR containing an exploit that I would need to first accept. Of course, that's [no guarantee nothing could slip in](https://www.puppet.com/blog/xz-backdoor), but it's certainly better than the wild west of asdf plugins. Given that asdf plugins rarely need patches of any kind it's relatively easy for me to audit each
change.

Most core tools and some aqua tools have support for extra security features like gpg verification, slsa-verify, cosign, or minisign. I try to adopt whatever the vendor provides to validate what gets fetched is genuine. This is an area that mise will perpetually need contributors helping though. If you notice a tool doesn't have any verification (you can see mise verifying during installs if you set `--verbose`), see if the vendor offers something like gpg signed checksums or slsa provenance. If so, it should just be a matter of
adding [configuration](https://aquaproj.github.io/docs/reference/security/cosign-slsa/) to the [aqua-registry](https://github.com/aquaproj/aqua-registry).

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

## Windows support

asdf does not run on Windows at all. With mise, tools using non-asdf backends can support Windows. Of course, this means the tool
vendor must provide Windows binaries but if they do, and the backend isn't asdf, the tool should work on Windows.

## Security

asdf plugins are insecure. They typically are written by individuals with no ties to the vendors that provide the underlying tool.
Where possible, mise does not use asdf plugins and instead uses backends like aqua and ubi which do not require separate plugins.

Aqua tools can be configured with cosign/slsa verification as well. See [SECURITY](https://github.com/jdx/mise/blob/main/SECURITY.md) for more information.

## Command Compatibility

In nearly all places you can use the exact syntax that works in asdf, however this likely won't
show up in the help or CLI reference. If you're coming from asdf and comfortable with that way of
working you can almost always use the same syntax with mise, e.g.:

```sh
mise install node 20.0.0
mise local node 20.0.0
```

UPDATE (2025-01-01): asdf-go (0.16+) actually got rid of `asdf global|local` entirely in favor of `asdf set` which we can't support since we already have a command named `mise set`. mise command compatibility will likely not be as good with asdf-go 0.16+.

It's not recommended though. You almost always want to modify config files and install things so
`mise use node@20` saves an extra command. Also, the "@" in the command is preferred since it allows
you to install multiple tools at once: `mise use|install node@20 node@18`. Also, there are edge cases
where it's not possible—or at least very challenging—for us to definitively know which syntax is being
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

## Other considerations

* [mise seems to be far more popular than asdf at least on homebrew](https://formulae.brew.sh/analytics/install-on-request/30d/)
* despite asdf having a 9-year lead—I believe I've put much more hours into mise than asdf developers have put into asdf. As a result, mise is pretty complete, you won't find issues/discussions with loads of +1's for mise like you will for [asdf](https://github.com/asdf-vm/asdf/issues?q=is%3Aissue+is%3Aopen+sort%3Areactions-%2B1-desc).
* mise's stargazer count is growing more rapidly—it's likely only a few more months until it overtakes asdf in terms of total stars:

![star-history-202511 (1)](https://github.com/user-attachments/assets/3f3d2484-99fb-434e-931b-4291f2f96761)

## asdf was a great tool, and I hope it becomes a great one again

I owe a lot to asdf. Without it mise either wouldn't exist or it wouldn't be nearly as good. The plugin model (while problematic for security reasons) is quite clever and I don't think I would've come up with something quite so elegant. I really appreciate the hard
work asdf developers have put in over the years. This doc feels like throwing shade and I hate that, but I also think it's more important to inform users that asdf is not a good choice anymore and they should know why.

While I think that asdf has a lot of problems (I wouldn't have started writing mise if that wasn't the case), my hope is that they address them and are able to make asdf into a great tool again.

I think asdf still has the edge in one important area: it's simpler. Users discovering mise are unsurprisingly intimidated by everything mise does. The simple fact that mise has a task runner is enough to put people off and I don't blame them. The docs for mise are huge and take a long time to go through. Even just the tools portion of mise is far more complex in every way (docs, code, config) than asdf. I think there are users out there that would prefer a more lightweight alternative to mise and I think asdf could be that but right now there are just too many problems. Even though it's lightweight, it's actually quite a bit more difficult to use.
