# About

mise (pronounced “meez”), short for _mise-en-place_, helps you set up and work in development
environments. The name comes from the French culinary practice of preparing ingredients and
utensils before cooking. In a project, `mise.toml` serves a similar purpose: it records the
tools, environment, and commands needed to get to work.

## What mise manages

- **[Development tools](/dev-tools/):** install runtimes and command-line tools, select versions
  per project, and share those choices with your team and CI.
- **[Environment variables](/environments/):** define project configuration, load dotenv files
  or secrets, and activate environments such as Python virtualenvs.
- **[Tasks](/tasks/):** give build, test, lint, and other commands names, dependencies, and arguments.
- **[Machine setup](/bootstrap.html):** declare packages, files, services, and other host setup
  separately from a project's tool installations.

You can adopt these features independently. Start by managing one tool or one task; you do
not need to move all your existing scripts and configuration at once.

## Where to start

Follow [Getting Started](/getting-started.html) for the first setup, then the
[walkthrough](/walkthrough.html) to work through a project. The [glossary](/glossary.html)
explains terms such as backend, shim, and task. Use [Troubleshooting](/troubleshooting.html)
when a command or shell environment does not behave as expected.

## Contact

mise was created by [Jeff Dickey](https://jdx.dev/) and is developed with its
[contributors](/team.html). The aim is to make development easier and more consistent across
languages. Questions, bug reports, and suggestions are welcome; see [Contact](/contact.html)
for the right place to start.
