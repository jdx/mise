// Old documentation URLs keep working after a page or a section moves.
//
// Both maps are append-only. Published links, search results, and released mise
// binaries point at these URLs, so an entry stays after the move ships. Keys and
// targets are root-absolute published paths: `/page.html`, `/section/` for an
// index page, and `/page.html#id` for a section. Moved sections live in
// anchor-redirects.mjs, which the browser loads only when a hash misses.

/**
 * Pages that no longer exist. The build writes a stub at each key that sends
 * the reader to `to`. When the old page's sections went to different places,
 * `hashes` maps an old section id to its new home; any other hash is carried
 * over to `to`.
 *
 * @type {Record<string, { to: string, hashes?: Record<string, string> }>}
 */
export const pageRedirects = {};

/**
 * Normalize a published path so `/a.html`, `/a`, `/dir/`, `/dir/index.html`
 * and `/dir` compare equal to their map keys.
 *
 * @param {string} path
 */
export function normalizePath(path) {
  const normalized = path
    .replace(/\.html$/, "")
    .replace(/\/index$/, "/")
    .replace(/\/+$/, "");
  return normalized === "" ? "/" : normalized;
}

/**
 * The file a page redirect's stub is written to, relative to the build output.
 *
 * @param {string} from
 */
export function stubFile(from) {
  if (from.endsWith("/")) return `${from.slice(1)}index.html`;
  if (from.endsWith(".html")) return from.slice(1);
  return `${from.slice(1)}.html`;
}

/**
 * Where a removed page sends a reader who arrived with `hash` (including `#`).
 *
 * @param {{ to: string, hashes?: Record<string, string> }} redirect
 * @param {string} hash
 */
export function pageRedirectTarget(redirect, hash) {
  const id = decodeHash(hash);
  if (id && redirect.hashes && Object.hasOwn(redirect.hashes, id)) {
    return redirect.hashes[id];
  }
  if (id && !redirect.to.includes("#")) return `${redirect.to}${hash}`;
  return redirect.to;
}

/**
 * Where a section that moved off `pathname` now lives, if it moved.
 *
 * @param {string} pathname
 * @param {string} hash including the leading `#`
 * @param {Record<string, string>} redirects
 */
export function anchorRedirectTarget(pathname, hash, redirects) {
  const id = decodeHash(hash);
  if (!id) return undefined;
  const page = normalizePath(pathname);
  for (const [from, to] of Object.entries(redirects)) {
    const [fromPath, fromId] = splitHash(from);
    if (fromId === id && normalizePath(fromPath) === page) return to;
  }
  return undefined;
}

/**
 * The static page written in place of a removed one. It works without
 * JavaScript (meta refresh, which drops the hash) and with it (the script also
 * maps the hash).
 *
 * @param {{ to: string, hashes?: Record<string, string> }} redirect
 * @param {string} siteUrl
 */
export function redirectStub(redirect, siteUrl) {
  const to = escapeHtml(redirect.to);
  const canonical = escapeHtml(new URL(redirect.to, siteUrl).toString());
  const script = `(function () {
  var to = ${scriptJson(redirect.to)};
  var hashes = ${scriptJson(redirect.hashes || {})};
  var id = location.hash.slice(1);
  try { id = decodeURIComponent(id); } catch (e) {}
  if (id && Object.prototype.hasOwnProperty.call(hashes, id)) to = hashes[id];
  else if (id && to.indexOf("#") < 0) to += location.hash;
  location.replace(to);
})();`;
  return `<!doctype html>
<html lang="en-US">
<head>
<meta charset="utf-8">
<title>Moved to ${to}</title>
<meta name="robots" content="noindex">
<link rel="canonical" href="${canonical}">
<script>${script}</script>
<meta http-equiv="refresh" content="0; url=${to}">
</head>
<body>
<p>This page moved to <a href="${to}">${to}</a>.</p>
</body>
</html>
`;
}

function decodeHash(hash) {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

function splitHash(url) {
  const index = url.indexOf("#");
  return index === -1 ? [url, ""] : [url.slice(0, index), url.slice(index + 1)];
}

function escapeHtml(text) {
  return text
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// JSON inside a <script> must not close the tag.
function scriptJson(value) {
  return JSON.stringify(value).replace(/</g, "\\u003c");
}
