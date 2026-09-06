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
| Website overview and entry points                 | `docs/index.md` and the hero in `docs/.vitepress/theme/HomeHero.vue`            |
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

### TOML examples

The docs use **TOML 1.1**. Multiline inline tables, comments inside them, and
trailing commas are valid. Keep that syntax when it makes an example easier to
read:

```toml
[tools]
node = {
  version = "24", # a version request, not an exact pin
}
```

Validate snippets with the repository's built mise (`mise fmt --stdin`) or
another TOML 1.1 parser. A TOML 1.0-only validator can incorrectly reject valid
examples. Parse each complete example separately, and label fragments that need
surrounding configuration. Alternative definitions of the same TOML key must
be separate examples or commented out.

## Generated content

Check for a generated-file comment before editing reference pages. For CLI changes,
edit the command documentation in `src/cli/`, then run `mise run render:usage`.
For settings, edit `settings.toml` and run `mise run render:schema` as described in
[AGENTS.md](https://github.com/jdx/mise/blob/main/AGENTS.md). Review generated diffs and keep unrelated changes out of
the patch.

Rebuild the LLM index with `mise run render:llms` after changing page titles,
introductory content, or the page list. It writes `docs/public/llms.txt` from the
source pages; do not maintain that index by hand. Regenerate it after rebasing
so it reflects the pages on the PR's base.

## Review a documentation change

Check formatting with the repository's lint tools, build the site, and inspect
changed pages in the browser. Check narrow and wide layouts when changing the
homepage or theme. Follow the new-reader path and verify that commands, filenames,
and expected results agree across the README and website. Check links to heading
anchors in the rendered page as well as page paths: generated settings IDs may
contain dots and underscores, while Markdown headings use VitePress's slug rules.
A successful build does not prove that every fragment link points to an ID.
