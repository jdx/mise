---
name: pr-description
description: How to write or revise a mise pull request title and description. PR titles and bodies feed the release notes, so use this whenever opening a PR, editing a PR body, or after review feedback changes what a PR does.
---

# Writing PR Titles and Descriptions

Communique uses PR titles and descriptions to generate release notes. Write them
for a mise user who has not read the diff or this conversation.

- **Describe the final result.** Before requesting review and again after feedback
  changes the implementation, compare the title and body with the complete current
  diff. Rewrite both when the scope changes. Remove abandoned approaches, stale
  requirements, and claims that the final code or validation no longer supports.
- **Lead with the user-visible change.** Keep the conventional commit format, but
  name the affected behavior and outcome in the title. Open the body with the
  problem or use case and what users can now do. Avoid titles such as "address
  feedback" or "fix CI" when the PR's actual purpose is a feature or behavior fix.
  For internal-only work, explain the concrete maintainer or contributor benefit
  without inventing a user-facing impact.
- **Make the change concrete.** For new configuration or commands, include a small,
  valid example and explain its result. For a bug fix, describe the trigger and
  before/after behavior. For visible UI or output changes, include actual before/after
  screenshots or a short recording when they help reviewers assess the change;
  CLI input/output snippets are often clearer than screenshots of a terminal.
  Use measured results for performance claims and state how they were measured.
- **Keep the essential facts in text.** Caption screenshots and explain examples.
  A reader or release-note generator should understand the change without opening
  an image, following an external link, or reading the diff. Do not fabricate
  screenshots, output, measurements, or validation results.
- **State adoption details when relevant.** Include new flags or settings, defaults,
  supported platforms, experimental status, required dependency versions, and any
  compatibility changes or migration steps that affect using the feature. Distinguish
  current behavior from planned follow-ups; do not advertise unfinished work.
- **Keep review details proportionate.** Summarize meaningful validation and its
  limitations. Include implementation details only when they explain behavior or a
  tradeoff reviewers need to assess. Omit agent work logs, intermediate commit
  summaries, and exhaustive test-command lists. A small fix can be a short paragraph
  and a test result; screenshots and sections are not mandatory for every PR.

For example, prefer `fix(task): install missing tools before running referenced tasks`
over `fix(task): address review feedback`. Its description should explain which
command previously failed, show the same command succeeding with automatic tool
installation, and mention any relevant prerequisites. This guidance does not replace
the AI disclosure that AGENTS.md requires on GitHub content.

## Use cases and tradeoffs

For user-facing behavior, explain materially different use cases or tradeoffs
(for example, bounded catalogs versus query-driven search). Write the description
for users and reviewers, not as a list of files or implementation steps; keep
implementation details and test commands in secondary sections.
