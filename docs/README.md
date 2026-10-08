---
description: "Preview and build the mise documentation site, and follow its rules for pages, examples, links, style, and generated reference content."
---

# Working on the mise docs

This directory holds the [mise documentation website](https://mise.jdx.dev/),
built with VitePress. This guide is for contributors; the site does not publish
it. Run the commands below from the repository root.

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

The task installs JavaScript dependencies, runs the social image and redirect
tests, and builds the site. It then checks the generated social images and
every internal link, including section anchors and links written in raw HTML or
Vue components. Use `mise run docs:preview` to serve the production build
locally.

To re-record `docs/tapes/demo.gif` and `docs/tapes/demo.mp4` after changing
`docs/tapes/demo.tape`, start Docker and run `mise run docs:demos`.

## Choose the right page

| Content                                           | Location                                                                           |
| ------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Project introduction and a short runnable example | Root `README.md`                                                                   |
| Website overview and entry points                 | `docs/index.md` and the hero in `docs/.vitepress/theme/HomeHero.vue`               |
| First tool, environment variable, and task        | `docs/getting-started.md`                                                          |
| Adding mise to an existing project                | `docs/walkthrough.md`                                                              |
| Shell activation and completions                  | `docs/shell-setup.md`                                                              |
| Feature guides                                    | Each feature's directory, such as `docs/dev-tools/`, and its overview page         |
| Configuration reference                           | `docs/configuration.md` and `docs/configuration/`                                  |
| Site navigation                                   | `docs/.vitepress/sidebar.ts`                                                       |
| Generated command reference                       | `docs/cli/`, generated from the sources in [Generated content](#generated-content) |

Put detailed behavior in the relevant feature guide and link to it from onboarding
pages. Keep the README short enough for someone deciding whether to try mise.

## Move or rename a page

- Changing a page's title or H1 keeps its URL. Make the page's label in
  `docs/.vitepress/sidebar.ts` match the new H1.
- Moving, renaming, or removing a page file changes its URL. Add an entry to
  `pageRedirects` in `docs/.vitepress/redirects.mjs` so the old URL sends
  readers to the new page.
- Moving a section to another page breaks links to its anchor. Add an
  `anchorRedirects` entry from `/old-page.html#old-id` to its new location.
- Rewording a heading changes its anchor. Keep the old anchor with an explicit
  ID such as `{#activate-mise}`. Released mise binaries print some docs URLs, so
  search `src/`, `crates/`, and `settings.toml` as well as `docs/` before you
  rename a heading.

## Write examples readers can run

- State prerequisites, the working directory, and whether shell activation is required.
- Include every file, tool, and dependency needed for a runnable example. Label excerpts and illustrations.
- Explain what a command changes: installing a tool, saving a version request, or running a process.
- Use a small observable result, such as printing an environment variable. Avoid live deployment as a first example.
- Distinguish version requests from exact pins and lockfile resolutions. Avoid output that becomes stale with each release.
- Keep platform-specific commands in labeled code groups. Use `mise exec` or `mise run` when activation is unnecessary.
- Use `packslip`, `aqua`, or `github` backends in examples, not the deprecated `ubi`.

Use descriptive headings and link text. Internal website links start at the
docs root and end in `.html`, for example `/dev-tools/backends/github.html`, or
in `/` for a section index, such as `/dev-tools/`.

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

## Voice and style

- Write for a developer who knows their shell but not mise. Use "you", the
  present tense, and the active voice, and describe what mise does rather than
  selling it.
- Write `mise` in lowercase, even at the start of a sentence, and use sentence
  case for headings.
- Give every page a frontmatter `description`: one sentence of 50 to 160
  characters that starts with a verb. The paragraph under the H1 becomes the
  page's summary in `llms.txt`, so make it a complete statement on its own.
- Say each fact once, where it belongs, and link to it from other pages.
- Cut throat-clearing openers ("This page explains"), recap sections, history
  asides about what an option used to do, filler ("Note that", "simply"), and
  marketing words ("powerful", "seamless", "modern").
- Do not use em dashes, bold-label bullet lists ("**Feature**: text"), or
  "will" for current behavior.
- Use VitePress containers such as `::: tip` and `::: warning` at most once per
  section, and never for a required step.

## Generated content

Never edit generated files: `docs/cli/`, `mise.usage.kdl`, `man/`,
`schema/*.json`, and `docs/public/llms.txt`. Check for a generated-file comment
before editing any reference page.

Each CLI page in `docs/cli/` combines three sources:

- The command's help text, arguments, flags, and examples come from its clap
  definition in `src/cli/`.
- Source-code links and argument completions come from
  `src/assets/mise-extra.usage.kdl`.
- The "Related documentation" links at the end of each page come from the
  `guides` map in `docs/.vitepress/cli-reference.ts`. Update it when you add a
  command or move the page a guide points to; the render fails when a guide
  points to a page that no longer exists.

After changing any of them, run `mise run render:usage`.

For settings, edit `settings.toml` and run `mise run render:schema` as described
in [AGENTS.md](https://github.com/jdx/mise/blob/main/AGENTS.md). The settings
page renders `settings.toml` when the site builds. Review generated diffs and
keep unrelated changes out of the patch.

Rebuild the LLM index with `mise run render:llms` after changing page titles,
introductory content, or the page list. It writes `docs/public/llms.txt` from the
source pages; do not maintain that index by hand. Regenerate it after rebasing
so it reflects the pages on the PR's base.

## Review a documentation change

Check formatting with the repository's lint tools, build the site, and inspect
changed pages in the browser. Check narrow and wide layouts when changing the
homepage or theme. Follow the new-reader path and verify that commands, filenames,
and expected results agree across the README and website. Settings anchors keep
the dots and underscores of the setting name, such as `#task.output`, while
Markdown headings use VitePress's slug rules; the build's link check reports a
link to an anchor that does not exist.

## The landing-page showreel

The homepage plays a showreel under the hero when the build has one. It is
rendered, never committed: `docs/public/showreel*.mp4` and
`docs/public/showreel-poster.jpg` are gitignored.

```sh
mise run docs:showreel                 # render it from the captures on this machine
mise run docs:showreel -- --capture    # record the terminal captures first if they are out of date
```

Rendering needs ffmpeg and Playwright's Chromium headless shell
(`aube exec playwright-core install chromium-headless-shell`); recording the
captures needs Docker. With a render in `docs/public`, `mise run docs:build`
adds the player and the homepage's `og:video`, and points "Watch the demo" at
it. To build the site without it, as the docs workflow does for pull requests,
delete `docs/public/showreel*.mp4` and `docs/public/showreel-poster.jpg`. For a
draft of part of the reel, run `aube run showreel:video --help`. The storyboard,
art spec, and capture rig are in `docs/.vitepress/theme/showreel/` and
`docs/.vitepress/showreel-capture/`.

You never need to commit or upload a render. The docs deploy renders the reel
itself and caches the result. It renders again when the reel's code, fonts,
song, renderer, or capture rig changes, or when the capture set's key changes
(the capture rig's README says what the key covers). To force a new recording
and render, run the docs workflow by hand with `rerender-showreel`.
