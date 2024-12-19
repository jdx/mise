# Mise + Ruby Cookbook

Here are some tips on managing Ruby projects with mise.

## A Ruby on Rails Project

```toml [mise.toml]
min_version = "2024.9.5"

[env]
# Project information
PROJECT_NAME = "{{ config_root | basename }}"

[tools]
# Install Ruby with the specified version
ruby = "{{ get_env(name='RUBY_VERSION', default='3.3.3') }}"

# Install gem dependencies
[tasks."bundle:install"]
run = "bundle install"

[tasks.server]
description = "Start the Rails server"
alias = "s"
run = "rails server"

[tasks.test]
description = "Run tests"
alias = "t"
run = "rails test"

[tasks.lint]
description = "Run lint using Rubocop"
alias = "l"
run = "rubocop"
```
