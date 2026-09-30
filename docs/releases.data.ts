import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseChangelog } from "./.vitepress/releases.mjs";

const docsDir = dirname(fileURLToPath(import.meta.url));
const changelogPath = resolve(docsDir, "../CHANGELOG.md");
const issuesPath = resolve(docsDir, ".vitepress/releases-issues.json");

export type Release = {
  version: string;
  /** ISO date, YYYY-MM-DD. */
  date: string;
  /** Changelog entries, leaving out new-contributor thanks and the vendored Aqua registry. */
  changes: number;
  categories: {
    features: number;
    fixes: number;
    registry: number;
    other: number;
  };
  /** Changelog entries by section, as in the release notes. */
  sections: {
    title: string;
    entries: {
      text: string;
      scope?: string;
      author?: string;
      /** "#123" for a pull request, a short hash for a commit. */
      ref?: string;
      /** Full hash, when ref is a commit. */
      commit?: string;
    }[];
  }[];
  /**
   * Issues closed by the release's pull requests. Null before issues came
   * back (ISSUES_SINCE), and for a newer release not counted yet.
   */
  issues: number | null;
};

declare const data: Release[];
export { data };

// Oldest first. The page draws a timeline left to right and lists newest first.
export default {
  watch: [changelogPath, issuesPath],
  load(): Release[] {
    const issues: Record<string, number> = JSON.parse(
      readFileSync(issuesPath, "utf8"),
    );
    return parseChangelog(readFileSync(changelogPath, "utf8"))
      .map(({ prs: _prs, ...release }) => ({
        ...release,
        issues: issues[release.version] ?? null,
      }))
      .reverse();
  },
};
