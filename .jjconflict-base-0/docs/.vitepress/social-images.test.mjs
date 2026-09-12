import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  socialCard,
  textWidth,
  wrapTitle,
  writeSocialCard,
} from "./social-images.mjs";
import { pageDescription } from "./social-descriptions.mjs";

test("descriptions require a non-empty string even when a subtitle is supplied", () => {
  for (const description of [undefined, null, "", " \n ", false, 123, [], {}]) {
    assert.throws(
      () =>
        pageDescription(
          { description, socialDescription: "Subtitle" },
          "guide.md",
        ),
      /frontmatter description: guide.md/,
    );
  }
  assert.equal(
    pageDescription({ description: "  Custom\nsummary.  " }, "guide.md"),
    "Custom summary.",
  );
});

test("subtitles are escaped, bounded, and invalidate the image URL", () => {
  const card = socialCard("Packslip", "Signed releases & verified <downloads>");
  assert.match(card.svg, /&amp;/);
  assert.match(card.svg, /&lt;downloads&gt;/);
  assert.notEqual(card.path, socialCard("Packslip", "Another summary").path);
  for (const description of [
    "Long description words ".repeat(30),
    "W".repeat(200),
  ]) {
    const rendered = socialCard("Very long heading ".repeat(30), description);
    assert.ok(rendered.subtitle.length <= 100);
    assert.ok(wrapTitle(rendered.subtitle, 32).length <= 2);
  }
});

test("long headings and command names stay out of the logo column", () => {
  for (const title of [
    "W".repeat(120),
    "Machine-wide compilation scheduling and memory budgets",
    "mise help --verbose",
  ]) {
    const lines = wrapTitle(title);
    assert.ok(lines.length > 0);
    for (const line of lines) assert.ok(textWidth(line, 62) <= 720, line);
  }
});

test("escapes title markup and gives changed content a new image URL", () => {
  const first = socialCard('Rust & C++ <build> "cache"');
  assert.match(first.svg, /Rust &amp; C\+\+/);
  assert.ok(!first.svg.includes("<build>"));
  assert.equal(first.path, socialCard('Rust & C++ <build> "cache"').path);
  assert.notEqual(first.path, socialCard("Another page").path);
});

test("renders a self-contained 1200 by 630 PNG", () => {
  const dir = mkdtempSync(join(tmpdir(), "mise-social-"));
  try {
    const card = socialCard("Get started");
    writeSocialCard(dir, card);
    const png = readFileSync(join(dir, card.path));
    assert.deepEqual(
      [...png.subarray(0, 8)],
      [137, 80, 78, 71, 13, 10, 26, 10],
    );
    assert.equal(png.readUInt32BE(16), 1200);
    assert.equal(png.readUInt32BE(20), 630);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("final long-title layout shrinks, truncates, and stays inside the title column", () => {
  const card = socialCard("Very long documentation heading ".repeat(30));
  const lines = [
    ...card.svg.matchAll(
      /<text x="64" y="([^"]+)" font-size="(\d+)" fill="#f4eee3">([^<]*)<\/text>/g,
    ),
  ];
  assert.equal(lines.length, 3);
  assert.ok(lines.at(-1)[3].endsWith("…"));
  for (const [, y, size, text] of lines) {
    assert.equal(Number(size), 54);
    assert.ok(textWidth(text, Number(size)) <= 720);
    assert.ok(Number(y) - Number(size) > 100);
    assert.ok(Number(y) <= 320);
  }
});

test("image URL hashes exactly the emitted PNG", () => {
  const card = socialCard("Cache invalidation");
  const hash = createHash("sha256").update(card.png).digest("hex").slice(0, 16);
  assert.equal(card.path, `social/${hash}.png`);
});

test("built-page checks reject swapped images and empty alt text", () => {
  const dir = mkdtempSync(join(tmpdir(), "social-validation-"));
  const first = socialCard("First page", "Description");
  const second = socialCard("Second page", "Description");
  const page = (title, card, alt = title + " — mise docs. Description") => `
    <script type="application/ld+json">${JSON.stringify({ "@type": "WebPage", name: title + " | mise-en-place", description: "Description", url: "https://example.com/page.html" })}</script>
    <meta name="description" content="Description">
    <meta property="og:image:type" content="image/png">
    <meta property="og:image:width" content="1200">
    <meta property="og:image:height" content="630">
    <link rel="canonical" href="https://example.com/page.html">
    <meta property="og:url" content="https://example.com/page.html">
    <meta property="og:title" content="${title} | mise-en-place">
    <meta name="twitter:title" content="${title} | mise-en-place">
    <meta property="og:description" content="Description">
    <meta name="twitter:description" content="Description">
    <meta property="og:image" content="https://example.com/${card.path}">
    <meta name="twitter:image" content="https://example.com/${card.path}">
    <meta property="og:image:alt" content="${alt}">
    <meta name="twitter:image:alt" content="${alt}">
    <meta name="twitter:card" content="summary_large_image">`;
  const check = () =>
    spawnSync(
      process.execPath,
      [
        fileURLToPath(new URL("./check-social-images.mjs", import.meta.url)),
        dir,
      ],
      { encoding: "utf8" },
    );
  try {
    writeSocialCard(dir, first);
    writeSocialCard(dir, second);
    writeFileSync(join(dir, "first.html"), page("First page", first));
    writeFileSync(join(dir, "second.html"), page("Second page", second));
    const valid = check();
    assert.equal(valid.status, 0, valid.stderr);
    writeFileSync(join(dir, "first.html"), page("First page", second));
    const swapped = check();
    assert.notEqual(swapped.status, 0);
    assert.match(swapped.stderr, /Wrong page image/);
    writeFileSync(join(dir, "first.html"), page("First page", first, ""));
    const empty = check();
    assert.notEqual(empty.status, 0);
    assert.match(empty.stderr, /Empty image alt text/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
