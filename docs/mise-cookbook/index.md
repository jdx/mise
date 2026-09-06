# Cookbook

These recipes combine tools, environment variables, and tasks for specific
workflows. Start with the recipe closest to your project, then adapt its paths,
versions, and application commands. Each recipe states the files or tools it
expects to exist.

| Workflow                                                       | Recipe                                                  |
| -------------------------------------------------------------- | ------------------------------------------------------- |
| Configure and build a CMake project                            | [C++](/mise-cookbook/cpp.html)                          |
| Install mise and share tools in containers                     | [Docker](/mise-cookbook/docker.html)                    |
| Run npm scripts or select a package manager                    | [Node.js](/mise-cookbook/nodejs.html)                   |
| Work with requirements files, uv projects, or inline scripts   | [Python](/mise-cookbook/python.html)                    |
| Run Rails and Bundler commands                                 | [Ruby](/mise-cookbook/ruby.html)                        |
| Initialize, validate, plan, and apply infrastructure changes   | [Terraform and OpenTofu](/mise-cookbook/terraform.html) |
| Highlight task scripts and configure embedded language servers | [Neovim](/mise-cookbook/neovim.html)                    |
| Create your own project scaffold                               | [Presets](/mise-cookbook/presets.html)                  |
| Customize prompts and inspect shell integration                | [Shell tricks](/mise-cookbook/shell-tricks.html)        |

For the underlying behavior, see [task configuration](/tasks/task-configuration.html),
[environment variables](/environments/), and [tool configuration](/dev-tools/).

## Contributing

Share a recipe in the [cookbook discussion](https://github.com/jdx/mise/discussions/3645).
Include prerequisites, a complete config, the command to run, and the expected
result so another reader can reproduce the workflow.
