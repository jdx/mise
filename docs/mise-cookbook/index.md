---
description: "Start from a working mise.toml for Node.js, Python, Rails, C++, Bazel, Terraform or Docker projects."
---

# Cookbook overview

Each recipe gives the config and commands for one kind of project or workflow.
Start from the closest one and change its versions, paths and commands to match
your project.

| Recipe                                                  | What it sets up                                                                  |
| ------------------------------------------------------- | -------------------------------------------------------------------------------- |
| [Tips and tricks](/tips-and-tricks.html)                | Short recipes for everyday commands, scripts, configuration and the shell prompt |
| [Bazel](/mise-cookbook/bazel.html)                      | Bazel from a project's `.bazelversion`, or Bazelisk                              |
| [C++ and CMake](/mise-cookbook/cpp.html)                | CMake with configure, build and clean tasks                                      |
| [Neovim](/mise-cookbook/neovim.html)                    | Highlighting for task scripts and language servers for embedded code             |
| [Node.js](/mise-cookbook/nodejs.html)                   | npm scripts as tasks, pnpm or aube, and package managers without Corepack        |
| [Python](/mise-cookbook/python.html)                    | A `requirements.txt` virtualenv, a uv project, and uv scripts                    |
| [Ruby on Rails](/mise-cookbook/ruby.html)               | Ruby from `.ruby-version` with Bundler and Rails tasks                           |
| [Scaffolding tasks](/mise-cookbook/presets.html)        | A global task that adds tools and tasks to a new project                         |
| [Terraform and OpenTofu](/mise-cookbook/terraform.html) | init, validate, plan and apply tasks for a subdirectory                          |
| [Docker](/mise-cookbook/docker.html)                    | mise in a container image, with tools installed during the build                 |

Several recipes define an install task for project packages. The experimental
[`mise deps`](/dev-tools/deps.html) can replace it and run the package manager
only when the manifest or lockfile changes, or when its outputs go missing.

For the reference behind the recipes, see [task configuration](/tasks/task-configuration.html),
[environment variables](/environments/) and [Dev tools](/dev-tools/).

## Contribute a recipe {#contributing}

Open a pull request that adds a page under `docs/mise-cookbook/`, or share the
recipe in the [cookbook discussion](https://github.com/jdx/mise/discussions/3645).
Include the prerequisites, a complete config, the command to run and the
expected result.
