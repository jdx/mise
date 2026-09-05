// Verify the built HTML references real, page-specific PNG previews.
import assert from "node:assert/strict";
import { socialCard } from "./social-images.mjs";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";

const root = resolve(process.argv[2] || ".vitepress/dist");
const meta = (html, key) => {
  const tags = [...html.matchAll(/<meta\b[^>]*>/g)].map(([tag]) =>
    Object.fromEntries(
      [...tag.matchAll(/([\w:-]+)=(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)].map(
        ([, name, quoted, single, bare]) => [name, quoted ?? single ?? bare],
      ),
    ),
  );
  const matches = tags.filter(
    (tag) => tag.property === key || tag.name === key,
  );
  assert.equal(matches.length, 1, `Expected one ${key} tag`);
  return matches[0].content;
};
const walk = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? walk(join(dir, entry.name)) : [join(dir, entry.name)],
  );
let posts = 0;
const images = new Set();
for (const file of walk(root).filter((file) => file.endsWith(".html"))) {
  const html = readFileSync(file, "utf8");
  assert.equal(meta(html, "og:title"), meta(html, "twitter:title"));
  assert.equal(meta(html, "og:description"), meta(html, "twitter:description"));
  // Check the actual page title rather than only counting distinct image URLs.
  const decode = (value) =>
    value.replace(/&(amp|lt|gt|quot|apos|#\d+|#x[\da-f]+);/gi, (_, entity) => {
      if (entity.startsWith("#x"))
        return String.fromCodePoint(parseInt(entity.slice(2), 16));
      if (entity.startsWith("#"))
        return String.fromCodePoint(Number(entity.slice(1)));
      return { amp: "&", lt: "<", gt: ">", quot: '\"', apos: "'" }[entity];
    });
  const pageTitle = decode(meta(html, "og:title")).replace(
    / \| mise-en-place$/,
    "",
  );
  const heading =
    file === join(root, "index.html")
      ? "Dev tools, environments, and tasks"
      : pageTitle;
  const alt = meta(html, "og:image:alt");
  assert.ok(
    typeof alt === "string" && alt.trim(),
    `Empty image alt text: ${file}`,
  );
  assert.equal(
    decode(alt),
    heading + " — mise docs",
    `Wrong image alt text: ${file}`,
  );
  assert.equal(meta(html, "twitter:card"), "summary_large_image");
  const expected = socialCard(heading);
  const image = meta(html, "og:image");
  assert.equal(meta(html, "twitter:image"), image);
  assert.equal(meta(html, "twitter:image:alt"), meta(html, "og:image:alt"));
  assert.equal(
    new URL(image).pathname,
    `/${expected.path}`,
    `Wrong page image: ${file}`,
  );
  assert.match(image, /^https:\/\//);
  assert.notEqual(new URL(image).pathname, "/og.png");
  const png = readFileSync(join(root, new URL(image).pathname));
  assert.deepEqual(png, expected.png, `Wrong image content: ${file}`);
  assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
  assert.equal(png.readUInt32BE(16), 1200);
  assert.equal(png.readUInt32BE(20), 630);
  images.add(image);
  posts++;
}
assert.ok(posts > 0, "No built pages found");
assert.ok(images.size > 1, "Pages should have distinct images");
console.log(
  `Checked images and social metadata for ${posts} documentation pages.`,
);
