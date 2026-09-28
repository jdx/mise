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
    <meta property="og:type" content="website">
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

test("built-page checks offer the showreel as og:video on the homepage only", () => {
  const dir = mkdtempSync(join(tmpdir(), "showreel-validation-"));
  const heading = "Dev tools, environments, and tasks";
  const home = socialCard(heading, "Description");
  const other = socialCard("Other page", "Description");
  // Just enough of an MP4 for the checks: a box size, then "ftyp".
  const mp4 = (brand) =>
    Buffer.concat([Buffer.from([0, 0, 0, 16]), Buffer.from(`ftyp${brand}`)]);
  const video = mp4("isom0060");
  const video120 = mp4("isom0120");
  const jpeg = Buffer.from([0xff, 0xd8, 0xff, 0xe0]);
  const version = (file) =>
    createHash("sha256").update(file).digest("hex").slice(0, 12);
  const src = `/showreel.mp4?v=${version(video)}`;
  const posterSrc = `/showreel-poster.jpg?v=${version(jpeg)}`;
  const tags = (url) => `
    <meta property="og:video" content="${url}">
    <meta property="og:video:secure_url" content="${url}">
    <meta property="og:video:type" content="video/mp4">
    <meta property="og:video:width" content="1920">
    <meta property="og:video:height" content="1080">`;
  const page = (
    path,
    title,
    card,
    { type = "website", extra = "", alt = title } = {},
  ) => `
    <script type="application/ld+json">${JSON.stringify({ "@type": "WebPage", name: title + " | mise-en-place", description: "Description", url: `https://example.com/${path}` })}</script>
    <meta name="description" content="Description">
    <meta property="og:type" content="${type}">${extra}
    <meta property="og:image:type" content="image/png">
    <meta property="og:image:width" content="1200">
    <meta property="og:image:height" content="630">
    <link rel="canonical" href="https://example.com/${path}">
    <meta property="og:url" content="https://example.com/${path}">
    <meta property="og:title" content="${title} | mise-en-place">
    <meta name="twitter:title" content="${title} | mise-en-place">
    <meta property="og:description" content="Description">
    <meta name="twitter:description" content="Description">
    <meta property="og:image" content="https://example.com/${card.path}">
    <meta name="twitter:image" content="https://example.com/${card.path}">
    <meta property="og:image:alt" content="${alt} — mise docs. Description">
    <meta name="twitter:image:alt" content="${alt} — mise docs. Description">
    <meta name="twitter:card" content="summary_large_image">`;
  const vtt = "WEBVTT\n";
  const player = (video = src, poster = posterSrc) => `
    <a class="action-btn" href="/#showreel">Watch the demo</a>
    <section id="showreel" class="home-showreel"><video src="${video}" poster="${poster}" preload="none"><track kind="chapters" src="/showreel-chapters.vtt?v=${version(Buffer.from(vtt))}"></video></section>`;
  const homeWith = (url, body = player()) =>
    page("", "Home", home, {
      type: "video.other",
      extra: tags(url),
      alt: heading,
    }) + body;
  const otherPage = (extra = "") =>
    page("other.html", "Other page", other, { extra });
  const check = () =>
    spawnSync(
      process.execPath,
      [
        fileURLToPath(new URL("./check-social-images.mjs", import.meta.url)),
        dir,
      ],
      { encoding: "utf8" },
    );
  const expectFailure = (pattern) => {
    const result = check();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, pattern);
  };
  try {
    writeSocialCard(dir, home);
    writeSocialCard(dir, other);
    writeFileSync(join(dir, "showreel.mp4"), video);
    writeFileSync(join(dir, "showreel-120.mp4"), video120);
    writeFileSync(join(dir, "showreel-poster.jpg"), jpeg);
    writeFileSync(join(dir, "showreel-chapters.vtt"), vtt);
    const upgrade = (v) =>
      writeFileSync(
        join(dir, "app.js"),
        `const upgrade = "/showreel-120.mp4?v=${v}";`,
      );
    upgrade(version(video120));
    const valid = () =>
      writeFileSync(
        join(dir, "index.html"),
        homeWith(`https://example.com${src}`),
      );
    valid();
    writeFileSync(join(dir, "other.html"), otherPage());
    const ok = check();
    assert.equal(ok.status, 0, ok.stderr);
    assert.match(ok.stdout, /with the showreel/);

    // A stale render's URL, in the tags, the player or its poster.
    writeFileSync(
      join(dir, "index.html"),
      homeWith("https://example.com/showreel.mp4?v=0123456789ab"),
    );
    expectFailure(/og:video is not the deployed showreel\.mp4/);
    writeFileSync(
      join(dir, "index.html"),
      homeWith(
        `https://example.com${src}`,
        player("/showreel.mp4?v=0123456789ab"),
      ),
    );
    expectFailure(/The player does not start on the deployed showreel\.mp4/);
    // The poster carries its own version, not the video's.
    writeFileSync(
      join(dir, "index.html"),
      homeWith(
        `https://example.com${src}`,
        player(src, `/showreel-poster.jpg?v=${version(video)}`),
      ),
    );
    expectFailure(/The player's poster is not the deployed render's/);
    // Watch the demo needs the player's anchor.
    writeFileSync(
      join(dir, "index.html"),
      homeWith(
        `https://example.com${src}`,
        player().replace(' id="showreel"', ""),
      ),
    );
    expectFailure(/no #showreel anchor/);
    valid();

    // Only the homepage is a video, and every page has one og:type.
    writeFileSync(
      join(dir, "other.html"),
      otherPage(tags(`https://example.com${src}`)),
    );
    expectFailure(/Unexpected og:video tags/);
    writeFileSync(
      join(dir, "other.html"),
      otherPage('<meta property="og:type" content="article">'),
    );
    expectFailure(/Expected one og:type tag/);
    writeFileSync(join(dir, "other.html"), otherPage());

    // The 120 fps file must be the one the player's script switches to.
    upgrade("0123456789ab");
    expectFailure(/No script plays \/showreel-120\.mp4/);
    upgrade(version(video120));

    writeFileSync(join(dir, "showreel-poster.jpg"), Buffer.from("PNG"));
    expectFailure(/showreel-poster\.jpg is not a JPEG/);
    writeFileSync(join(dir, "showreel-poster.jpg"), jpeg);

    // Without a render, the homepage is a website with no player, and
    // nothing links to the missing anchor.
    rmSync(join(dir, "showreel.mp4"));
    expectFailure(/Wrong og:type/);
    const plain = page("", "Home", home, { alt: heading });
    writeFileSync(
      join(dir, "index.html"),
      plain + `<video src="${src}"></video>`,
    );
    expectFailure(/The homepage has a player but no showreel\.mp4/);
    writeFileSync(
      join(dir, "index.html"),
      plain + '<a href="/#showreel">Watch the demo</a>',
    );
    expectFailure(/A link to \/#showreel, but no showreel\.mp4/);
    writeFileSync(
      join(dir, "index.html"),
      plain + '<a href="/demo">Watch the demo</a>',
    );
    const without = check();
    assert.equal(without.status, 0, without.stderr);
    assert.doesNotMatch(without.stdout, /with the showreel/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
