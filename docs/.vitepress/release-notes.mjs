// The GitHub release notes the Releases page shows (the communique version,
// not the changelog).
//
// release-plz snapshots every published release's notes into release-notes/
// (see release-notes-sync.mjs), so a docs build reads them from the repo. The
// newest release has no snapshot yet when its release PR is opened, so a docs
// build fetches whichever releases in CHANGELOG.md have no file. The docs
// workflow also runs when a release is published, which is when that notes
// body exists.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseChangelog } from "./releases.mjs";

const here = dirname(fileURLToPath(import.meta.url));
export const notesDir = resolve(here, "../../release-notes");
const changelogPath = resolve(here, "../../CHANGELOG.md");

// A build that finds more than this many new releases without notes has a
// snapshot that is broken or was never made; it does not go and fetch them all.
const MAX_FETCH = 10;

// release.yml appends this block to every release body. It is the same on
// every release, so it is left out of the notes.
const SPONSOR = /^## (?:💚 )?Sponsor mise[ \t]*$/m;

/**
 * A release's notes as stored in release-notes/<version>.md: the release title
 * as a first-line heading (when it has one), then the body.
 *
 * @param {{name?: string|null, tag_name: string, body?: string|null}} release
 *   a release from the GitHub API
 */
export function formatNotes(release) {
  let body = (release.body ?? "").replace(/\r\n/g, "\n");
  const sponsor = SPONSOR.exec(body);
  if (sponsor) body = body.slice(0, sponsor.index);
  body = body.trim();
  if (!body) return null;
  // The name is "v2026.9.17: Title"; a release with no title is named by its tag.
  const title = (release.name ?? "")
    .replace(new RegExp(`^${escapeRegExp(release.tag_name)}:?\\s*`), "")
    .trim();
  return `${title ? `# ${title}\n\n` : ""}${body}\n`;
}

/** The inverse of formatNotes: { title, markdown }. */
export function parseNotes(text) {
  const m = /^# (.+)\n\n/.exec(text);
  return m
    ? { title: m[1], markdown: text.slice(m[0].length) }
    : { title: "", markdown: text };
}

const escapeRegExp = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Notes kept in the repo, as Map<version, {title, markdown}>. */
export function committedNotes() {
  const notes = new Map();
  let files = [];
  try {
    files = readdirSync(notesDir);
  } catch {
    return notes;
  }
  for (const file of files) {
    if (!file.endsWith(".md")) continue;
    notes.set(
      file.slice(0, -3),
      parseNotes(readFileSync(resolve(notesDir, file), "utf8")),
    );
  }
  return notes;
}

async function fetchRelease(version) {
  const url = `https://api.github.com/repos/jdx/mise/releases/tags/v${version}`;
  const token =
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN ||
    process.env.MISE_GITHUB_TOKEN;
  // A token that GitHub rejects (an expired one in the environment) must not
  // fail a build that can go without it, so a 401 is retried without it.
  for (const auth of token ? [true, false] : [false]) {
    const res = await fetch(url, {
      headers: {
        accept: "application/vnd.github+json",
        ...(auth ? { authorization: `Bearer ${token}` } : {}),
      },
    });
    if (res.status === 401 && auth) continue;
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    return res.json();
  }
}

let cached;

/**
 * Notes for every release in CHANGELOG.md that has some, as
 * Map<version, {title, markdown}>. A release whose notes cannot be fetched is
 * left out, and the page links to GitHub for it instead.
 */
export function releaseNotes() {
  cached ??= (async () => {
    const notes = committedNotes();
    // Only releases newer than the newest snapshot are fetched. A few older
    // ones never had a GitHub release (their tags were skipped), so they have no
    // file and never will.
    const missing = [];
    for (const { version } of parseChangelog(
      readFileSync(changelogPath, "utf8"),
    )) {
      if (notes.has(version)) break;
      missing.push(version);
    }
    if (missing.length > MAX_FETCH) {
      console.warn(
        `release-notes: ${missing.length} new releases have no snapshot in release-notes/; not fetching them`,
      );
      return notes;
    }
    await Promise.all(
      missing.map(async (version) => {
        try {
          const text = formatNotes(await fetchRelease(version));
          if (text) notes.set(version, parseNotes(text));
        } catch (err) {
          // Expected while a release is still being prepared: it has no
          // GitHub release yet.
          console.warn(
            `release-notes: no notes for ${version} (${err.message})`,
          );
        }
      }),
    );
    return notes;
  })();
  return cached;
}
