// Generates docs/public/llms.txt — a machine-readable index of the docs, served
// at https://mise.jdx.dev/llms.txt. Coding agents fetch this path to discover
// what documentation exists before deciding what to read.
//
// This writes no prose of its own. The page list comes from the VitePress
// sidebar and each description is the lead paragraph of the page itself, so the
// index cannot describe pages differently from how they describe themselves.
//
// Run via `mise run render:llms`.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { sidebar, type SidebarItem } from "./sidebar";

const configDir = dirname(fileURLToPath(import.meta.url));
const docsDir = resolve(configDir, "..");
const outFile = resolve(docsDir, "public/llms.txt");

// Matches the canonical URL built in config.ts.
const SITE_URL = "https://mise.jdx.dev";

const MAX_DESCRIPTION = 200;

/** `/dev-tools/` -> `docs/dev-tools/index.md`, `/demo` -> `docs/demo.md` */
function sourceFile(link: string): string {
  const rel = link.replace(/^\//, "");
  return resolve(docsDir, rel.endsWith("/") ? `${rel}index.md` : `${rel}.md`);
}

/** `/demo` -> `https://mise.jdx.dev/demo.html` (VitePress does not use cleanUrls) */
function pageUrl(link: string): string {
  return link.endsWith("/") ? `${SITE_URL}${link}` : `${SITE_URL}${link}.html`;
}

/** Strip the markdown that would only add noise for a reader of this index. */
function plain(md: string): string {
  return md
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "") // images
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1") // links -> text
    .replace(/`([^`]*)`/g, "$1")
    .replace(/[*_]{1,3}([^*_]+)[*_]{1,3}/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * The lead paragraph of a page: the first prose block after the H1, skipping
 * frontmatter, VitePress containers, badges and code. Pages that open with a
 * blockquote summary (`> ...`) use that.
 */
function description(file: string): string | undefined {
  let md: string;
  try {
    md = readFileSync(file, "utf8");
  } catch {
    return undefined;
  }

  md = md.replace(/^---\n[\s\S]*?\n---\n/, ""); // frontmatter

  let seenHeading = false;
  const paragraph: string[] = [];

  for (const raw of md.split("\n")) {
    const line = raw.trim();

    if (!seenHeading) {
      if (line.startsWith("# ")) seenHeading = true;
      continue;
    }
    if (paragraph.length === 0) {
      // Skip anything before the first prose block.
      if (
        line === "" ||
        line.startsWith("#") ||
        line.startsWith(":::") ||
        line.startsWith("```") ||
        line.startsWith("<") ||
        line.startsWith("|") ||
        line.startsWith("- ") ||
        line.startsWith("* ") ||
        // `<script setup>` bodies for pages that embed a Vue component
        line.startsWith("import ") ||
        line.startsWith("export ")
      ) {
        continue;
      }
    } else if (line === "" || line.startsWith("#") || line.startsWith(":::")) {
      break;
    }
    paragraph.push(line.replace(/^>\s?/, ""));
  }

  // Many lead paragraphs introduce a list or code block, so they end in a colon.
  const text = plain(paragraph.join(" ")).replace(/:$/, ".");
  if (!text) return undefined;
  if (text.length <= MAX_DESCRIPTION) return text;

  // Prefer cutting at a sentence boundary, else a word boundary.
  const sentence = text.slice(0, MAX_DESCRIPTION).lastIndexOf(". ");
  if (sentence > MAX_DESCRIPTION / 2) return text.slice(0, sentence + 1);
  return `${text.slice(0, text.lastIndexOf(" ", MAX_DESCRIPTION))}…`;
}

type Entry = { text: string; link: string };

function flatten(items: SidebarItem[], into: Entry[] = []): Entry[] {
  for (const item of items) {
    if (item.link?.startsWith("/"))
      into.push({ text: item.text, link: item.link });
    if (item.items) flatten(item.items, into);
  }
  return into;
}

const missing: string[] = [];
const sections: string[] = [];

for (const group of sidebar) {
  const entries = flatten(group.items ?? []);
  if (group.link?.startsWith("/")) {
    entries.unshift({ text: group.text, link: group.link });
  }
  if (entries.length === 0) continue;

  const lines = entries.map(({ text, link }) => {
    const file = sourceFile(link);
    const desc = description(file);
    if (!desc) missing.push(link);
    return desc
      ? `- [${text}](${pageUrl(link)}): ${desc}`
      : `- [${text}](${pageUrl(link)})`;
  });

  sections.push(`## ${group.text}\n\n${lines.join("\n")}`);
}

const out = `# mise

> mise-en-place is a polyglot dev tool version manager, environment variable manager, and task runner. It installs and switches between versions of tools like node, python, and go, loads project environment variables, and runs project tasks.

This file indexes the mise documentation for coding agents. Every page below is
the same page a human reads — there is no separate agent documentation. Fetch a
page for the detail; do not guess at flags, settings, or config keys.

${sections.join("\n\n")}
`;

writeFileSync(outFile, out);

const pages = out.split("\n").filter((l) => l.startsWith("- ")).length;
console.log(`wrote ${outFile} (${pages} pages, ${out.length} bytes)`);
if (missing.length > 0) {
  console.log(`no lead paragraph found for ${missing.length} page(s):`);
  for (const link of missing) console.log(`  ${link}`);
}
