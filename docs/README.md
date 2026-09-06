# Working on the mise docs

This directory contains the [mise documentation website](https://mise.jdx.dev/),
built with VitePress. Run the commands below from the repository root.

## Preview and build

Install the repository's development tools with `mise install`, then start the site:

```sh
mise run docs
```

Open the local URL printed by VitePress. Edits reload automatically.

Before submitting a change, build the production site:

```sh
mise run docs:build
```

The task installs JavaScript dependencies, runs the social image tests, builds the
site, and checks generated social images. VitePress also checks internal page links.
Use `mise run docs:preview` to serve the production build locally.

## Choose the right page

| Content                                           | Location                                                                        |
| ------------------------------------------------- | ------------------------------------------------------------------------------- |
| Project introduction and a short runnable example | Root `README.md`                                                                |
| Website overview and entry points                 | `docs/index.md` and the hero in `docs/.vitepress/theme/Layout.vue`              |
| First successful tool, environment, and task      | `docs/getting-started.md`                                                       |
| Daily use, configuration choices, and upgrades    | `docs/walkthrough.md`                                                           |
| Concepts and feature guides                       | `docs/dev-tools/`, `docs/environments/`, `docs/tasks/`, and `docs/bootstrap.md` |
| Configuration reference                           | `docs/configuration.md` and `docs/configuration/`                               |
| Site navigation                                   | `docs/.vitepress/sidebar.ts`                                                    |
| Generated command reference                       | `docs/cli/`                                                                     |

Put detailed behavior in the relevant feature guide and link to it from onboarding
pages. Keep the README short enough for someone deciding whether to try mise.

## Write examples readers can run

- State prerequisites, the working directory, and whether shell activation is required.
- Include every file, tool, and dependency needed for a runnable example. Label excerpts and illustrations.
- Explain what a command changes: installing a tool, saving a version request, or running a process.
- Use a small observable result, such as printing an environment variable. Avoid live deployment as a first example.
- Distinguish version requests from exact pins and lockfile resolutions. Avoid output that becomes stale with each release.
- Keep platform-specific commands in labeled code groups. Use `mise exec` or `mise run` when activation is unnecessary.

Use descriptive headings and link text. Preserve existing heading anchors when
reorganizing a page, using explicit IDs such as `{#activate-mise}` where needed.
Internal website links start at the docs root, for example
`/dev-tools/backends/github.html`.

## Generated content

Check for a generated-file comment before editing reference pages. For CLI changes,
edit the command documentation in `src/cli/`, then run `mise run render:usage`.
For settings, edit `settings.toml` and run `mise run render:schema` as described in
[AGENTS.md](https://github.com/jdx/mise/blob/main/AGENTS.md). Review generated diffs and keep unrelated changes out of
the patch.

## Review a documentation change

Check formatting with the repository's lint tools, build the site, and inspect
changed pages in the browser. Check narrow and wide layouts when changing the
homepage or theme. Follow the new-reader path and verify that commands, filenames,
and expected results agree across the README and website.
