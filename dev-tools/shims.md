# Shims

::: tip
The [beginner's guide](https://dev.to/jdxcode/beginners-guide-to-rtx-ac4), and my [blog post](https://jdx.dev/posts/2024-04-13-shims-how-they-work-in-mise-en-place/) are helpful resources to dive deeper into shims.
:::

While the PATH design of mise works great in most cases, there are some situations where shims are
preferable. One example is when calling mise binaries from an IDE.

To support this, mise does have a shim dir that can be used. It's located at `~/.local/share/mise/shims`.

```sh
$ mise use -g node@20
$ npm install -g prettier@3.1.0
$ mise reshim # may be required if new shims need to be created after installing packages
$ ~/.local/share/mise/shims/node -v
v20.0.0
$ ~/.local/share/mise/shims/prettier -v
3.1.0
```

::: tip
`mise activate --shims` is a shorthand for adding the shims directory to PATH.
:::

::: info
`mise reshim` actually should get called automatically if you're using npm so an explicit reshim should not be necessary
in that scenario. Also, this bears repeating but: `mise reshim` just creates/removes the shims. People use it as a
"fix it" button but it really should only be necessary if `~/.local/share/mise/shims` doesn't contain something it should.

mise also runs a reshim anytime a tool is installed/updated/removed so you don't need to use it for those scenarios.

Also don't put things in there manually, mise will just delete it next reshim.
:::

## Shims vs PATH

In general, I recommend using PATH (`mise activate`) instead of shims for _interactive_ situations. The
way activate works is every time the prompt is displayed, mise-en-place will determine what PATH and other
env vars should be and export them. This is why it doesn't work well for non-interactive situations like
scripts. The prompt never gets displayed so you have to manually call `mise hook-env` to get mise to update
the env vars.

Also, if you run a set of commands in a single line like the following:

```sh
$ cd ~
$ cd ~/src/proj1 && node -v && cd ~/src/proj2 && node -v
```

Using `mise activate`, this will use the tools from `~`, not from `~/src/proj1` or `~/src/proj2` even
after the directory changed because the prompt never got displayed. That might be obvious to you, not sure,
what I'm trying to convey though is just think of mise running just before your prompt gets displayed—because
that literally is what is happening. It's not a magical utility that is capable of having your environment
always setup perfectly in every situation even though it might normally "feel" that way.

Note that shims _will_ work with the inline example above.

::: info
This may be fixable at least for some shells if they support a hook for directory change, however
some investigation will need to be done. See [#1294](https://github.com/jdx/mise/issues/1294) for details.
:::

### `which`

`which` is a command that I personally find great value in. shims effectively "break" `which` and
cause it to show the location of the shim. Of course `mise which` will show the location but I prefer
the "cleanliness" of running `which node` and getting back a real path with a version number inside of it.
e.g:

```sh
$ which node
/Users/jdx/.mise/installs/node/20/bin/node
```

### Env vars and shims

A downside of shims is the "mise environment" is only loaded when a shim is called. This means if you
set an environment variable in `.mise.toml`, it will only be run when a shim is called. So the following
only works under `mise activate`:

```sh
$ mise set NODE_ENV=production
$ echo $NODE_ENV
production
```

But this will work in either:

```sh
$ mise set NODE_ENV=production
$ node -p process.env.NODE_ENV
production
```

Also, `mise x|exec` and `mise r|run` can be used to get the environment even if you don't need any mise
tools:

```sh
$ mise set NODE_ENV=production
$ mise x -- bash -c "echo \$NODE_ENV"
production
$ mise r some_task_that_uses_NODE_ENV
production
```

::: tip
In general, [tasks](/tasks/) are a good way to ensure that the mise environment is always loaded so
this isn't a problem.
:::

## Hook on `cd`

Some version managers modify the behavior of `cd`. That might seem like the ideal method of making a version
manager, it has tons of gaps. It doesn't work if you use `pushd|popd` or other commands that modify PWD—though
some shells have a "chpwd" hook that would. It doesn't run if you modify the `.mise.toml` file.

The upside is that it doesn't run as frequently but since mise is written in rust the cost for executing
mise is negligible (~4-5ms).

## .zshrc/.bashrc files

rc files like `.zshrc` are unusual. It's a script but also runs only for interactive sessions. If you need
to access tools provided by mise inside of an rc file you have 2 options:

::: code-group
```sh [hook-env]
eval "$(mise activate zsh)"
eval "$(mise hook-env -s zsh)"
node some_script.js
```
```sh [shims]
eval "$(mise activate zsh --shims)" # should be first
eval "$(mise activate zsh)"
node some_script.js
```
:::

The `hook-env` option is the one I would go with. It's a bit cleaner since you won't have the shims
inside your PATH at all. If you do go with shims, it will need to be first so they get overridden.

## Performance

Truthfully, you're probably not going to notice much in the way of performance with any solution here.
However, I would like to document what the tradeoffs are since it's not as simple as "shims are slow".
In asdf they are, but that's because asdf is written in bash. In mise the cost of the shims are negligible.

First, since mise runs every time the prompt is displayed with `mise activate`, you'll pay a few ms cost
every time the prompt is displayed. Regardless of whether or not you're actively using a mise tool, you'll
pay that penalty every time you run any command. It does have some short-circuiting logic to make it faster
if there are no changes but it doesn't help much unless you have a very complex setup.

shims have basically the same performance profile but run when the shim is called. This makes some situations
better, and some worse.

If you are calling a shim from within a bash script like this:

```sh
for i in {1..500}; do
    node script.js
done
```

You'll pay the mise penalty every time you call it within the loop. However, if you did the same thing
but call a subprocess from within a shim (say, node creating a node subprocess), you will _not_ pay a new
penalty. This is because when a shim is called, mise sets up the environment with PATH for all tools and
those PATH entries will be before the shim directory.

In other words, which is better in terms of performance just depends on how you're calling mise. Really
though I think most users won't notice a 5ms lag on their terminal so I suggest `mise activate`.

## Neither shims nor PATH

[I don't actually use either of these methods](https://mise.jdx.dev/how-i-use-mise.html). There are many
ways to load the mise environment that don't require either, chiefly: `mise x|exec` and `mise r|run`.

These will both load all of the tools and env vars before executing something. I find this to be
ideal because I don't need to modify my shell rc file at all and my environment is always loaded
explicitly. I find this a "clean" way of working.

The obvious downside is that anytime I want to use `mise` I need to prefix it with `mise exec|run`,
though I alias them to `mx|mr`.

This is what I'd recommend if you're like me and prefer things to be precise over "easy". Or perhaps
if you're just wanting to use mise on a single project because that's what your team uses and prefer
not to use it to manage anything else on your system. IMO using a shell extension for that use-case
would be overkill.

Part of the reason for this is I often need to make sure I'm on my development version of mise. If you
work on mise yourself I would recommend working in a similar way and disabling `mise activate` or shims
while you are working on it.
