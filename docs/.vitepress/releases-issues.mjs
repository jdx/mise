// Counts the issues each release resolved and records them in
// releases-issues.json, which the releases page reads. An issue counts as
// resolved by a release when a pull request in that release's changelog
// closed it (GitHub's closingIssuesReferences: "Fixes #123", or a link made in
// the sidebar). Run by xtasks/release-plz on every release; it only looks up
// releases the file does not have yet, so a normal run is one small query.
//
//   node docs/.vitepress/releases-issues.mjs            # add missing releases
//   node docs/.vitepress/releases-issues.mjs --refresh  # recount everything
//
// Needs the gh CLI, signed in.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ISSUES_SINCE, parseChangelog } from "./releases.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const changelogPath = resolve(here, "../../CHANGELOG.md");
const issuesPath = resolve(here, "releases-issues.json");
const BATCH = 50;

function closedBy(prs) {
  const fields = prs
    .map(
      (n) =>
        `p${n}: pullRequest(number: ${n}) { closingIssuesReferences(first: 50) { nodes { id } } }`,
    )
    .join("\n");
  const query = `query { repository(owner: "jdx", name: "mise") { ${fields} } }`;
  for (let attempt = 1; ; attempt++) {
    try {
      const out = execFileSync(
        "gh",
        ["api", "graphql", "-f", `query=${query}`],
        { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
      );
      const repo = JSON.parse(out).data.repository;
      const issues = new Map();
      for (const n of prs) {
        // A null entry is a number that is not a pull request (a typo in a
        // changelog line); it closed nothing. Issues are told apart by node id,
        // not number: a pull request may close an issue in another repository,
        // and two of those could share a number.
        issues.set(
          n,
          (repo[`p${n}`]?.closingIssuesReferences.nodes ?? []).map((i) => i.id),
        );
      }
      return issues;
    } catch (err) {
      if (attempt === 4) throw err;
    }
  }
}

function readIssues() {
  try {
    return JSON.parse(readFileSync(issuesPath, "utf8"));
  } catch {
    return {};
  }
}

const refresh = process.argv.includes("--refresh");
// Earlier releases have no count: issues were off, so there is nothing to show.
const releases = parseChangelog(readFileSync(changelogPath, "utf8")).filter(
  (r) => r.date >= ISSUES_SINCE,
);
const counts = refresh ? {} : readIssues();
const todo = releases.filter((r) => !(r.version in counts));

const prs = [...new Set(todo.flatMap((r) => r.prs))];
const byPr = new Map();
for (let i = 0; i < prs.length; i += BATCH) {
  for (const [n, issues] of closedBy(prs.slice(i, i + BATCH))) {
    byPr.set(n, issues);
  }
}
for (const r of todo) {
  counts[r.version] = new Set(r.prs.flatMap((n) => byPr.get(n))).size;
}

// Same order as the changelog, newest first, so a diff shows only new lines.
const sorted = Object.fromEntries(
  releases
    .filter((r) => r.version in counts)
    .map((r) => [r.version, counts[r.version]]),
);
writeFileSync(issuesPath, JSON.stringify(sorted, null, 2) + "\n");
console.log(
  `releases-issues: ${todo.length} release(s) counted, ${Object.keys(sorted).length} total`,
);
