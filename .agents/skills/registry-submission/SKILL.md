---
name: registry-submission
description: Rules for adding a new tool or shorthand to mise's `registry/` — the popularity bar, the required user warning and popularity check, the PR's Popularity section, and which backends are accepted. Use before creating any new registry entry or registry PR. Not needed for maintenance of existing entries.
---

# Registry Submissions

**Most new registry additions from agents get rejected.** Before adding a new tool to `registry/`, understand the rules:

- **mise does not host self-written, personal, niche, or low-popularity tools.** The registry is curated for tools that are _already_ widely used. "It works" or "it has tests" is not the bar.
- **There is a high bar for new registry additions: tools generally need thousands of GitHub stars, not hundreds.** jdx will reject projects that do not meet this popularity bar and will not give a reason. Per [contributing.md](../../../docs/contributing.md): "@jdx won't explain why a given tool wasn't accepted." There is no appeal, no checklist, no second chance — the PR is closed and that's it.
- **Wasted PRs are the default outcome** for tools the agent or user has not vetted against this bar. Do not submit one speculatively.

## Required check for new registry additions

Before adding a new tool or shorthand to `registry/`, ALWAYS do the following. This check does not apply when editing an existing registry entry; do not ask the user about popularity for maintenance or fixes to tools already in the registry.

1. **Warn the user clearly and ask:** "New registry additions have a high popularity bar: jdx generally rejects projects without thousands of GitHub stars and will not give a reason. Is this tool already widely used outside your own projects and does it meet that bar?" If it's the user's own tool, a fork, an internal/company tool, or something with a small audience, **stop and tell them the PR will be rejected.** Do not submit.
2. **Actively check popularity for every new registry addition — no exceptions.** Look up real numbers; do not guess. Useful sources:
   - GitHub stars and fork count (`gh repo view owner/repo --json stargazerCount,forkCount`)
   - Recent release activity / last commit date (`gh repo view owner/repo --json pushedAt,latestRelease`)
   - Download counts on relevant package registries (npm `npmjs.com/package/x`, crates.io, PyPI, Homebrew analytics, etc.)
   - Whether the project shows up in third-party docs, awesome-lists, or other tools
3. **Apply the bar.** Rough signals — very low numbers are disqualifying:
   - GitHub stars in the thousands, not hundreds
   - Active maintenance (recent releases, not abandoned)
   - Real third-party usage (referenced in docs, blog posts, other tools, package registries)
   - Recognizable in its ecosystem
4. **Include the popularity data in the PR description.** Every PR adding a new registry tool or shorthand MUST contain a short section like:

   ```
   ## Popularity
   - GitHub: 12.3k stars, 480 forks, last release 2026-04-12
   - crates.io: 1.2M downloads
   - Used by: <project A>, <project B>
   ```

   This is non-negotiable for new additions — it lets the maintainer evaluate the submission without re-doing the research. New-tool PRs without it look speculative and are more likely to be rejected.

5. **If the tool is borderline or numbers are low, warn the user clearly** that the PR is likely to be rejected without reason, and ask if they still want to proceed. Do not soften this — users have repeatedly been surprised when their PR was closed, and the agent should have warned them up front.
6. **Suggest the alternative:** users can install any tool themselves via explicit backend syntax (`mise use aqua:owner/repo`, `mise use github:owner/repo`, `mise use cargo:name`, `mise use npm:name`, etc.) or by writing a [tool plugin](https://mise.jdx.dev/tool-plugin-development.html). The registry is _only_ for shorthand convenience for popular tools — not for enabling installation.

## Backend choice: packslip (preferred), then aqua, github, or gitlab

For registry entries the backend tiers are:

- **Version listing is mandatory.** Before adding a registry entry, run `mise ls-remote <backend>` and confirm it returns installable versions. A backend that can install only an explicitly pinned version is not sufficient, even if the package exists in an upstream registry. If the preferred backend cannot list versions, use another accepted backend (for example, a custom `http:` backend with a reliable `version_list_url`) or stop.
- **Tier 1 — preferred:** `packslip:`. Use it when the project publishes signed release manifests; mise verifies the signer and artifact digests without requiring a plugin or separate package manager.
- **Tier 2 — routinely accepted:** `aqua:`, `github:`, and `gitlab:`.
  - **Prefer `aqua:`** when the project does not publish packslips and the tool is in the [aqua registry](https://github.com/aquaproj/aqua-registry). Better UX, SLSA verification, and per-version logic.
  - **Use `github:`** when the tool isn't in aqua but ships GitHub releases.
  - **Use `gitlab:`** for tools released through GitLab.
- **Tier 3 — high bar, but lower than tier 4:** `conda:`. Potentially acceptable when the tool can't be supported via packslip/aqua/github/gitlab. The bar is lower than tier 4 because **the conda backend in mise does not require a separately-installed package manager** — mise downloads and extracts packages directly from anaconda.org via rattler, so users don't need conda/mamba on PATH. Still requires a popular, well-maintained tool.
- **Tier 4 — extremely high bar, almost never accepted:** `npm:`, `pipx:`, `gem:`, `cargo:`, `go:`, `dotnet:`. These all rely on a separately-installed runtime/toolchain being present on PATH (`node`, `python`, `ruby`, `cargo`, `go`, `dotnet`), which is fragile — the wrong version, a missing install, or PATH ordering quirks all break them. `npm:`/`pipx:`/`gem:` are particularly painful because tools installed via them silently bind to whichever `node`/`python`/`ruby` was on PATH at install time. Don't reach for these for a registry PR unless the user has explicitly confirmed @jdx wants it that way for this specific tool.
- **Not accepted at all:**
  - **New `asdf:` plugins** — supply-chain security. Use packslip/aqua/github/gitlab instead.
  - **New `vfox:` plugins** — same reason. Use packslip/aqua/github/gitlab instead.
  - **`ubi:`** is deprecated and will not be accepted under any circumstances.

Users can still install via any backend themselves with explicit syntax (`mise use vfox:...`, `mise use cargo:...`, etc.) — they just don't get a registry shorthand for it.
