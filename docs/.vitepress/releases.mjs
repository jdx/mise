// Reads CHANGELOG.md into one record per release. Shared by the docs data
// loader (releases.data.ts) and the script that looks up issue counts
// (releases-issues.mjs), so both agree on what a release contains.

const HEADING = /^## \[([^\]]+)\](?:\([^)]*\))? - (\d{4}-\d{2}-\d{2})\s*$/;
const PR_LINK = /\[#(\d+)\]\(https:\/\/github\.com\/jdx\/mise\/pull\/\d+\)/g;

/**
 * The day GitHub Issues were turned back on for the repository. The first
 * issue after the gap was opened on 2026-09-23; before that, reports came in
 * as Discussions, which a pull request cannot close. Releases from this date
 * on have an issue count, earlier ones do not.
 */
export const ISSUES_SINCE = "2026-09-23";

// A changelog line: "- **(scope)** text by @author in [#123](url)". Scope,
// author and reference are each optional (new-contributor thanks have no
// author; chores link a commit instead of a pull request).
const ENTRY =
  /^- (?:\*\*\(([^)]*)\)\*\* )?(.*?)(?: by @([\w[\]-]+))?(?: in \[([^\]]+)\]\(([^)]+)\))?$/;
const MD_LINK = /\[([^\]]*)\]\([^)]*\)/g;

/** One changelog line as { scope, text, author, ref, commit }. */
export function parseEntry(line) {
  const m = ENTRY.exec(line);
  const [, scope, text, author, ref, url] = m;
  const entry = { text: text.replace(MD_LINK, "$1") };
  if (scope) entry.scope = scope;
  if (author) entry.author = author;
  if (ref) {
    entry.ref = ref;
    // A commit reference is shown as its short hash and linked by its full one.
    if (!ref.startsWith("#")) entry.commit = url.split("/").pop();
  }
  return entry;
}

// Sections that are not changes to mise itself. New Contributors is a
// thank-you list; the Aqua sections list upstream registry updates that mise
// vendors, which would otherwise dwarf every real change.
const IGNORED_SECTIONS = /^(New Contributors|.*Aqua Registry.*)$/i;

/**
 * Bucket a changelog section title into one of the groups the releases page
 * draws. Section titles carry an emoji prefix and have changed over time, so
 * match on the words.
 */
export function categoryOf(title) {
  const t = title.toLowerCase();
  if (t.includes("feature")) return "features";
  if (t.includes("bug fix")) return "fixes";
  if (t.includes("registry")) return "registry";
  return "other";
}

/**
 * @param {string} text contents of CHANGELOG.md
 * @returns {{version: string, date: string, changes: number,
 *   categories: {features: number, fixes: number, registry: number, other: number},
 *   prs: number[],
 *   sections: {title: string, entries: ReturnType<typeof parseEntry>[]}[]}[]}
 *   newest first, as the file is. Sections leave out the vendored Aqua registry
 *   updates, which are a list of upstream packages rather than notes.
 */
export function parseChangelog(text) {
  const releases = [];
  let release = null;
  let section = null;
  for (const line of text.split("\n")) {
    const heading = HEADING.exec(line);
    if (heading) {
      release = {
        version: heading[1],
        date: heading[2],
        changes: 0,
        categories: { features: 0, fixes: 0, registry: 0, other: 0 },
        prs: [],
        sections: [],
      };
      releases.push(release);
      section = null;
      continue;
    }
    if (!release) continue;
    if (line.startsWith("## ")) {
      // A heading that is not a release (none today) ends the current one.
      release = null;
      continue;
    }
    if (line.startsWith("### ")) {
      section = line.slice(4).trim();
      if (!/aqua registry/i.test(section)) {
        release.sections.push({ title: section, entries: [] });
      }
      continue;
    }
    if (!line.startsWith("- ") || section === null) continue;
    if (!/aqua registry/i.test(section)) {
      release.sections[release.sections.length - 1].entries.push(
        parseEntry(line),
      );
    }
    if (IGNORED_SECTIONS.test(section)) continue;
    release.changes++;
    release.categories[categoryOf(section)]++;
    for (const m of line.matchAll(PR_LINK)) {
      const pr = Number(m[1]);
      if (!release.prs.includes(pr)) release.prs.push(pr);
    }
  }
  return releases;
}
