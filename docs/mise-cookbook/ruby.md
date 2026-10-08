---
description: "Select Ruby from a Rails app's .ruby-version and run its Bundler and Rails commands as mise tasks."
---

# Ruby on Rails

Pin Ruby for an existing Rails app from its `.ruby-version` file, and run the
app's Bundler and Rails commands as tasks. The recipe assumes `Gemfile`,
`Gemfile.lock`, `bin/rails` and `.ruby-version`, with RuboCop in the
development group.

## Use the app's `.ruby-version` {#a-ruby-on-rails-project}

```toml [mise.toml]
[settings]
idiomatic_version_file_enable_tools = ["ruby"]

[tasks.install]
description = "Install gem dependencies"
run = "bundle install"

[tasks.server]
description = "Start the Rails server"
alias = "s"
run = "bin/rails server"

[tasks.test]
description = "Run tests"
alias = "t"
run = "bin/rails test"

[tasks.lint]
description = "Run RuboCop"
alias = "l"
run = "bundle exec rubocop"
```

The setting makes mise read the app's `.ruby-version` (a leading `ruby-`, as in
`ruby-3.4.7`, is ignored), so the version stays in one file that other Ruby
tools also read. Because the setting is in the project's `mise.toml`, teammates
and CI read `.ruby-version` too. If the app has no `.ruby-version`, set
`ruby = "4.0"` under `[tools]` instead. See
[version files](/lang/ruby.html#version-files) for the other files mise can
read.

Run `mise run install` after cloning; mise installs the Ruby version first if it
is missing. Then run `mise run test` or `mise run server`. The binstubs and
`bundle exec` load the app's bundle, so Rails and RuboCop run at their locked
versions. Database setup stays with the app's own `bin/setup` or
`bin/rails db:prepare`.

mise installs precompiled Ruby where a build exists for your platform. For
source builds and the system libraries they need, see
[compiling with ruby-build](/lang/ruby.html#compiling-with-ruby-build).

To run `bundle install` only when `Gemfile` or `Gemfile.lock` changes, use the
experimental [`[deps.bundler]`](/dev-tools/deps.html) provider instead of the
install task.
