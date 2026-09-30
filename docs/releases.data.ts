import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parseChangelog } from "./.vitepress/releases.mjs";

const changelogPath = resolve(__dirname, "../CHANGELOG.md");
const issuesPath = resolve(__dirname, ".vitepress/releases-issues.json");

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
  /** Issues closed by the release's pull requests; null when not counted yet. */
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
