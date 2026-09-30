// Snapshots the notes of every published GitHub release listed in
// CHANGELOG.md into release-notes/<version>.md, which the docs build reads (see
// release-notes.mjs). Run by xtasks/release-plz on every release; it rewrites
// a file only when the notes changed, so a normal run changes nothing but the
// release published since the last one.
//
//   node docs/.vitepress/release-notes-sync.mjs
//
// Needs the gh CLI, signed in.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { formatNotes, notesDir } from "./release-notes.mjs";
import { parseChangelog } from "./releases.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const versions = new Set(
  parseChangelog(readFileSync(resolve(here, "../../CHANGELOG.md"), "utf8")).map(
    (r) => r.version,
  ),
);

const out = execFileSync(
  "gh",
  ["api", "--paginate", "--slurp", "repos/jdx/mise/releases?per_page=100"],
  { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
);
// --slurp collects the pages into one array of arrays.
const releases = JSON.parse(out).flat();

mkdirSync(notesDir, { recursive: true });
let written = 0;
for (const release of releases) {
  if (release.draft) continue;
  const version = release.tag_name.replace(/^v/, "");
  if (!versions.has(version)) continue;
  const text = formatNotes(release);
  if (!text) continue;
  const file = resolve(notesDir, `${version}.md`);
  if (existsSync(file) && readFileSync(file, "utf8") === text) continue;
  writeFileSync(file, text);
  written++;
}
console.log(`release-notes: wrote ${written} file(s)`);
