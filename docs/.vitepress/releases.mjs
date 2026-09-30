// Reads CHANGELOG.md into one record per release. Shared by the docs data
// loader (releases.data.ts) and the script that looks up issue counts
// (releases-issues.mjs), so both agree on what a release contains.

const HEADING = /^## \[([^\]]+)\](?:\([^)]*\))? - (\d{4}-\d{2}-\d{2})\s*$/;
const PR_LINK = /\[#(\d+)\]\(https:\/\/github\.com\/jdx\/mise\/pull\/\d+\)/g;

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
 *   prs: number[]}[]} newest first, as the file is
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
      continue;
    }
    if (!line.startsWith("- ") || section === null) continue;
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
