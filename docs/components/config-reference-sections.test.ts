import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { allGuides, guideFor } from "./config-reference-sections.ts";

const docs = new URL("../", import.meta.url);

// How VitePress turns a heading into an id (@mdit-vue/shared's slugify).
function slug(heading: string): string {
  return heading
    .replace(/<[^>]*>/g, "")
    .replace(/`/g, "")
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[\s~!@#$%^&*()\-_+=[\]{}|\;:"'“”‘’–—<>,.?/]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/^(\d)/, "_$1")
    .toLowerCase();
}

// The ids a page's headings have, from `{#id}` or the heading's text.
function anchors(markdown: string): Set<string> {
  const ids = new Set<string>();
  let fenced = false;
  for (const line of markdown.split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
    if (fenced) continue;
    const heading = line.match(/^#{1,6}\s+(.*?)\s*$/);
    if (!heading) continue;
    const explicit = heading[1].match(/^(.*?)\s*\{#([^}]+)\}$/);
    ids.add(explicit ? explicit[2] : slug(heading[1]));
  }
  return ids;
}

function pageOf(link: string): string {
  const path = link.replace(/#.*$/, "").replace(/^\//, "");
  return path === "" || path.endsWith("/")
    ? `${path}index.md`
    : path.replace(/\.html$/, ".md");
}

test("every guide leads to a page and heading that exist", () => {
  const missing: string[] = [];
  for (const { link } of allGuides()) {
    const file = new URL(pageOf(link), docs);
    if (!existsSync(file)) {
      missing.push(`${link}: no page ${pageOf(link)}`);
      continue;
    }
    const hash = link.includes("#") ? link.slice(link.indexOf("#") + 1) : "";
    if (hash && !anchors(readFileSync(file, "utf-8")).has(hash)) {
      missing.push(`${link}: no heading #${hash}`);
    }
  }
  assert.deepEqual(missing, []);
});

test("a key's guide is the nearest one above it, then its section's", () => {
  assert.equal(
    guideFor("tasks.<name>.depends")?.link,
    "/tasks/task-configuration.html#depends",
  );
  assert.equal(
    guideFor("tasks.<name>.depends.args")?.link,
    "/tasks/task-configuration.html#depends",
  );
  assert.equal(
    guideFor("tools.<tool>.os")?.link,
    "/dev-tools/#os-specific-tools",
  );
  assert.equal(
    guideFor("tools.<tool>.platforms")?.link,
    "/dev-tools/#tool-options",
  );
  assert.equal(guideFor("daemons.<name>.run")?.link, "/daemons.html");
  assert.equal(
    guideFor("watch_files[].patterns")?.link,
    "/hooks.html#watch-files-hook",
  );
  assert.equal(
    guideFor("oci.copy[].host")?.link,
    "/dev-tools/mise-oci.html#copy-files-into-the-image",
  );
  assert.equal(guideFor("nothing"), undefined);
});
