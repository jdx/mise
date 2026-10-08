// Fail the docs build on internal links that VitePress does not check: links to
// section anchors that no longer exist, links written as raw HTML or in Vue
// components, links to pages that only exist as redirect stubs, and redirect
// entries whose targets are gone.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { anchorRedirects } from "./anchor-redirects.mjs";
import { normalizePath, pageRedirects, stubFile } from "./redirects.mjs";

// Pages whose anchors are created in the browser: the releases page opens the
// release named in the hash.
const DYNAMIC_ANCHOR_PAGES = new Set(["/releases"]);

/** Read every HTML page under `root` into path -> { ids, links }. */
export function readSite(root) {
  const pages = new Map();
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const file = join(dir, entry.name);
      if (entry.isDirectory()) visit(file);
      else if (entry.name.endsWith(".html")) {
        const rel = relative(root, file).split("\\").join("/");
        const html = readFileSync(file, "utf8");
        pages.set(rel, {
          path: `/${rel}`,
          ids: new Set(
            [...html.matchAll(/\sid="([^"]+)"/g)].map(([, id]) =>
              decodeEntities(id),
            ),
          ),
          links: [...html.matchAll(/<a\b[^>]*?\shref="([^"]*)"/g)].map(
            ([, href]) => decodeEntities(href),
          ),
        });
      }
    }
  };
  visit(root);
  return pages;
}

/**
 * Check every internal link and redirect entry. Returns a list of problems.
 *
 * @param {Map<string, { path: string, ids: Set<string>, links: string[] }>} site
 */
export function checkSite(
  site,
  {
    pages = pageRedirects,
    anchors = anchorRedirects,
    dynamicAnchors = DYNAMIC_ANCHOR_PAGES,
  } = {},
) {
  const stubs = new Set(Object.keys(pages).map(stubFile));
  const byPath = new Map();
  for (const [file, page] of site) {
    if (!stubs.has(file)) byPath.set(normalizePath(page.path), page);
  }
  const redirected = new Map(
    Object.entries(pages).map(([from, redirect]) => [
      normalizePath(from),
      redirect.to,
    ]),
  );
  const movedSection = (path, id) => {
    for (const [from, to] of Object.entries(anchors)) {
      const [fromPath, fromId] = splitHash(from);
      if (fromId === id && normalizePath(fromPath) === path) return to;
    }
    return undefined;
  };

  const problems = [];
  // `where` names the page a link was found on; `target` is the link itself.
  const checkTarget = (where, target, { allowStub = false } = {}) => {
    const [pathPart, id] = splitHash(target);
    const path = normalizePath(pathPart);
    const page = byPath.get(path);
    if (!page) {
      const to = redirected.get(path);
      if (to && !allowStub) {
        problems.push(`${where}: links to moved page ${target}; link ${to}`);
      } else if (!to) {
        problems.push(`${where}: links to missing page ${target}`);
      }
      return;
    }
    if (id && !page.ids.has(id) && !dynamicAnchors.has(path)) {
      const to = movedSection(path, id);
      problems.push(
        to
          ? `${where}: links to moved section ${target}; link ${to}`
          : `${where}: links to missing anchor ${target}`,
      );
    }
  };

  for (const [file, page] of site) {
    if (stubs.has(file)) continue;
    const seen = new Set();
    for (const href of page.links) {
      const target = internalTarget(page.path, href);
      if (!target || seen.has(target)) continue;
      seen.add(target);
      checkTarget(page.path, target);
    }
  }

  for (const [from, redirect] of Object.entries(pages)) {
    if (!site.has(stubFile(from))) {
      problems.push(`redirects.mjs: no stub was written for ${from}`);
    }
    for (const to of [redirect.to, ...Object.values(redirect.hashes || {})]) {
      if (isInternal(to)) checkTarget(`redirect ${from}`, to);
    }
  }
  for (const [from, to] of Object.entries(anchors)) {
    const [fromPath, fromId] = splitHash(from);
    const page = byPath.get(normalizePath(fromPath));
    if (!page) {
      problems.push(
        `redirects.mjs: ${from} is on a page that does not exist; use pageRedirects`,
      );
    } else if (page.ids.has(fromId)) {
      problems.push(
        `redirects.mjs: ${from} still exists on its page, so the redirect never applies`,
      );
    }
    if (isInternal(to)) checkTarget(`redirect ${from}`, to);
  }
  return problems;
}

/** Resolve an <a href> on `pagePath` to a root-absolute page link, or null. */
export function internalTarget(pagePath, href) {
  if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//")) {
    return null;
  }
  const url = new URL(href, `https://docs.invalid${pagePath}`);
  if (url.host !== "docs.invalid") return null;
  const path = safeDecode(url.pathname);
  // Assets (schemas, images, llms.txt) are not pages.
  const last = path.split("/").pop();
  if (last.includes(".") && !last.endsWith(".html")) return null;
  const id = url.hash ? safeDecode(url.hash.slice(1)) : "";
  return id ? `${path}#${id}` : path;
}

// A malformed percent-escape is checked as written instead of crashing.
function safeDecode(text) {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

function isInternal(url) {
  return url.startsWith("/") && !url.startsWith("//");
}

function splitHash(url) {
  const index = url.indexOf("#");
  return index === -1 ? [url, ""] : [url.slice(0, index), url.slice(index + 1)];
}

function decodeEntities(text) {
  return text
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

if (import.meta.url === pathToFileURL(process.argv[1] || "").href) {
  const root = resolve(process.argv[2] || "docs/.vitepress/dist");
  const problems = checkSite(readSite(root));
  if (problems.length) {
    console.error(problems.join("\n"));
    console.error(`\n${problems.length} broken documentation links`);
    process.exit(1);
  }
  console.log("Checked documentation links and redirects.");
}
