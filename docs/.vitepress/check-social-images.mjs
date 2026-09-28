// Verify the built HTML references real, page-specific PNG previews, that only
// the homepage offers the rendered showreel as og:video, and that its player
// plays the files deployed with it.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { socialCard } from "./social-images.mjs";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";

const root = resolve(process.argv[2] || ".vitepress/dist");
const metaTags = (html) =>
  [...html.matchAll(/<meta\b[^>]*>/g)].map(([tag]) =>
    Object.fromEntries(
      [...tag.matchAll(/([\w:-]+)=(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)].map(
        ([, name, quoted, single, bare]) => [name, quoted ?? single ?? bare],
      ),
    ),
  );
const meta = (html, key) => {
  const matches = metaTags(html).filter(
    (tag) => tag.property === key || tag.name === key,
  );
  assert.equal(matches.length, 1, `Expected one ${key} tag`);
  return matches[0].content;
};
// Present only when `mise run docs:showreel` ran before the build.
const videoFile = join(root, "showreel.mp4");
const video = existsSync(videoFile) ? readFileSync(videoFile) : null;
const video120File = join(root, "showreel-120.mp4");
const video120 = existsSync(video120File) ? readFileSync(video120File) : null;
// The version must change with the file, or players and previews keep a
// stale render.
const version = (file) =>
  createHash("sha256").update(file).digest("hex").slice(0, 12);
// The landing page's player shows this until someone presses play.
const poster = video ? readFileSync(join(root, "showreel-poster.jpg")) : null;
if (poster) {
  assert.deepEqual(
    [...poster.subarray(0, 3)],
    [0xff, 0xd8, 0xff],
    "showreel-poster.jpg is not a JPEG",
  );
}
const walk = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? walk(join(dir, entry.name)) : [join(dir, entry.name)],
  );
let posts = 0;
const images = new Set();
for (const file of walk(root).filter((file) => file.endsWith(".html"))) {
  const html = readFileSync(file, "utf8");
  const home = file === join(root, "index.html");
  assert.equal(meta(html, "og:title"), meta(html, "twitter:title"));
  assert.equal(meta(html, "og:description"), meta(html, "twitter:description"));
  assert.equal(meta(html, "description"), meta(html, "og:description"));
  assert.ok(meta(html, "description").trim(), `Empty description: ${file}`);
  assert.equal(meta(html, "og:image:type"), "image/png");
  assert.equal(meta(html, "og:image:width"), "1200");
  assert.equal(meta(html, "og:image:height"), "630");
  const canonicals = [...html.matchAll(/<link\b[^>]*rel="canonical"[^>]*>/g)];
  if (file === join(root, "404.html")) {
    assert.equal(meta(html, "robots"), "noindex");
  } else {
    assert.equal(canonicals.length, 1, `Expected one canonical: ${file}`);
    assert.equal(
      canonicals[0][0].match(/href="([^"]+)"/)[1],
      meta(html, "og:url"),
    );
  }
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
  const structured = [
    ...html.matchAll(
      /<script\b[^>]*type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/g,
    ),
  ]
    .map(([, json]) => JSON.parse(json))
    .filter((data) => data["@type"] === "WebPage");
  assert.equal(structured.length, 1, `Expected one WebPage schema: ${file}`);
  assert.equal(structured[0].description, decode(meta(html, "description")));
  assert.equal(structured[0].name, decode(meta(html, "og:title")));
  assert.equal(structured[0].url, decode(meta(html, "og:url")));
  const heading = home ? "Dev tools, environments, and tasks" : pageTitle;
  const alt = meta(html, "og:image:alt");
  assert.ok(
    typeof alt === "string" && alt.trim(),
    `Empty image alt text: ${file}`,
  );
  const prefix = heading + " — mise docs. ";
  assert.ok(decode(alt).startsWith(prefix), `Wrong image alt text: ${file}`);
  const subtitle = decode(alt).slice(prefix.length);
  assert.ok(subtitle.trim(), `Empty subtitle: ${file}`);
  // X ignores og:video, so even the homepage keeps the large image card.
  assert.equal(meta(html, "twitter:card"), "summary_large_image");
  const expected = socialCard(heading, subtitle);
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

  // meta() also asserts there is exactly one og:type.
  const videoTags = metaTags(html).filter((tag) =>
    tag.property?.startsWith("og:video"),
  );
  if (video && home) {
    assert.equal(
      meta(html, "og:type"),
      "video.other",
      `Wrong og:type: ${file}`,
    );
    const url = meta(html, "og:video");
    assert.equal(meta(html, "og:video:secure_url"), url);
    assert.equal(meta(html, "og:video:type"), "video/mp4");
    assert.equal(meta(html, "og:video:width"), "1920");
    assert.equal(meta(html, "og:video:height"), "1080");
    assert.match(url, /^https:\/\//);
    assert.equal(new URL(url).pathname, "/showreel.mp4");
    assert.equal(
      new URL(url).searchParams.get("v"),
      version(video),
      "og:video is not the deployed showreel.mp4",
    );
    assert.equal(
      video.toString("latin1", 4, 8),
      "ftyp",
      "showreel.mp4 is not an MP4",
    );
    // The player starts on the same 60 fps file, with the poster rendered
    // with it, each versioned by its own bytes, and its chapters track.
    const player = html.match(/<video\b[^>]*\ssrc="([^"]*)"/);
    assert.equal(
      player?.[1],
      `/showreel.mp4?v=${version(video)}`,
      "The player does not start on the deployed showreel.mp4",
    );
    const posterSrc = html.match(/<video\b[^>]*\sposter="([^"]*)"/);
    assert.equal(
      posterSrc?.[1],
      `/showreel-poster.jpg?v=${version(poster)}`,
      "The player's poster is not the deployed render's",
    );
    const track = html.match(/<track\b[^>]*\ssrc="([^"]*)"/);
    const trackUrl = track && new URL(track[1], "https://mise.jdx.dev");
    const trackFile = trackUrl && join(root, trackUrl.pathname);
    assert.ok(
      trackFile && existsSync(trackFile),
      "The player's chapters track is not deployed",
    );
    assert.equal(
      trackUrl.searchParams.get("v"),
      version(readFileSync(trackFile)),
      "The player's chapters track is not versioned by its own bytes",
    );
    assert.match(
      html,
      /<section\b[^>]*\sid="showreel"/,
      "The player has no #showreel anchor for Watch the demo",
    );
    assert.match(
      html,
      /href="\/#showreel"/,
      "Watch the demo does not go to the player",
    );
  } else {
    assert.equal(meta(html, "og:type"), "website", `Wrong og:type: ${file}`);
    assert.equal(videoTags.length, 0, `Unexpected og:video tags in ${file}`);
    if (home) {
      assert.doesNotMatch(
        html,
        /<video\b/,
        "The homepage has a player but no showreel.mp4",
      );
    }
  }
  // "Watch the demo" goes to the player only when there is one.
  if (!video) {
    assert.doesNotMatch(
      html,
      /href="\/#showreel"/,
      `A link to /#showreel, but no showreel.mp4: ${file}`,
    );
  }
  posts++;
}
assert.ok(posts > 0, "No built pages found");
assert.ok(images.size > 1, "Pages should have distinct images");
if (video && video120) {
  // The player's script switches to the 120 fps file where it decodes well.
  assert.equal(
    video120.toString("latin1", 4, 8),
    "ftyp",
    "showreel-120.mp4 is not an MP4",
  );
  const src = `/showreel-120.mp4?v=${version(video120)}`;
  const scripts = walk(root).filter((file) => file.endsWith(".js"));
  assert.ok(
    scripts.some((file) => readFileSync(file, "utf8").includes(src)),
    `No script plays ${src}`,
  );
}
console.log(
  `Checked images and social metadata for ${posts} documentation pages${video ? ", with the showreel" : ""}.`,
);
