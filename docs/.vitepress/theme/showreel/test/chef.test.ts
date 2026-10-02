// The chef and the name card (kit/chef.ts, kit/namecard.ts). The block
// chef and the vector chef are copies of src/assets/logo.txt and
// docs/public/logo-dark.svg, so a change to either file fails here until
// the copies follow; the block chef is also held to what `mise` printed in
// C1's help screen. In Chromium: the settled vector chef against the SVG as
// the browser renders it, the toque's measured geometry against the SVG's
// own lines, the mosaic landing on the chef, effects that stay inside it,
// and frames that do not depend on the frame before them. The name card's
// copy is held to its sources, and the end card's moments to the reel's
// eighth-beat grid.
//
// The Chromium tests skip when no Chromium is found, unless
// SHOWREEL_REQUIRE_CHROMIUM is set; the capture test skips without the
// capture set, unless SHOWREEL_REQUIRE_CAPTURES is set.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import { BEAT, sec, TERM } from "../bible";
import { capture, rowRuns, rowText } from "../captures";
import { FONTS } from "../fonts";
import {
  bloomAt,
  CHEF_CELLS,
  CHEF_PATHS,
  cellLands,
  cellReveal,
  HALO,
  LOGO_COLS,
  LOGO_ROWS,
  LOGO_TXT,
  MORPH_CUE,
  morphState,
  OPEN_CUE,
} from "../kit/chef";
import {
  END_CUES,
  endCardState,
  INSTALL,
  PLATFORM,
  TAGLINE,
  URL,
} from "../kit/namecard";
import { beats, CHEF_ART, DUR } from "../kit/style";
import { loadFacts } from "../load";
import { REPO, SHOWREEL } from "./repo";

// The sources.

test("the block chef is src/assets/logo.txt, line for line", () => {
  const file = readFileSync(join(REPO, "src/assets/logo.txt"), "utf8");
  const lines = file.replace(/\n$/, "").split("\n");
  assert.deepEqual(
    [...LOGO_TXT],
    lines,
    "src/assets/logo.txt changed: copy it into kit/chef.ts LOGO_TXT",
  );
  assert.equal(LOGO_COLS, CHEF_ART.block.cols);
  assert.equal(LOGO_ROWS, CHEF_ART.block.rows);
});

test("the vector chef is docs/public/logo-dark.svg's paths, as the file has them", () => {
  const svg = readFileSync(join(REPO, "docs/public/logo-dark.svg"), "utf8");
  const ds = [...svg.matchAll(/<path\b[^>]*\sd="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(
    [...CHEF_PATHS],
    ds,
    "docs/public/logo-dark.svg changed: copy its paths into kit/chef.ts CHEF_PATHS",
  );
  const vb = /viewBox="([^"]+)"/.exec(svg)?.[1].split(/\s+/).map(Number);
  const V = CHEF_ART.viewBox;
  assert.deepEqual(vb, [V.x, V.y, V.w, V.h], "the viewBox moved");
  // One white fill, nonzero, no strokes, transforms or clips to reproduce.
  assert.match(svg, /fill:\s*#fff/);
  assert.doesNotMatch(svg, /stroke|transform=|clip-path|fill-rule|evenodd/);
  // The outline the backing fills is the third path's first subpath.
  assert.equal(CHEF_PATHS[2].slice(1).match(/M/g)?.length, 6);
});

test("the block chef parses into whole cells: every character a block or a space", () => {
  assert.equal(CHEF_CELLS.length, 117);
  for (const c of CHEF_CELLS) {
    const p = c.piece;
    assert.ok(
      p.x >= 0 && p.y >= 0 && p.x + p.w <= 1 && p.y + p.h <= 1,
      `row ${c.row} col ${c.col}`,
    );
    assert.ok(c.qcol >= 0 && c.qcol < 2 * LOGO_COLS);
  }
  // The wink (▔) is the one piece that is not a whole quadrant.
  const eighths = CHEF_CELLS.filter(
    (c) => c.piece.w !== 0.5 || c.piece.h !== 0.5,
  );
  assert.deepEqual(
    eighths.map((c) => [c.row, c.col, c.piece.h]),
    [[5, 9, 1 / 8]],
  );
});

const { facts, report } = loadFacts(REPO);

test("the block chef is what `mise` printed: C1's rows 1–7, columns 66–79, in green", (t) => {
  const c = capture(facts, "C1");
  if (!c) {
    if (process.env.SHOWREEL_REQUIRE_CAPTURES) assert.fail(report.join("; "));
    t.skip(`no capture set: ${report.join("; ")}`);
    return;
  }
  const B = CHEF_ART.block;
  const last = c.frames[c.frames.length - 1];
  LOGO_TXT.forEach((line, r) => {
    const row = last.rows[B.row + r];
    const text = Array.from(rowText(c, row));
    const got = text
      .slice(B.col, B.col + B.cols)
      .join("")
      .trimEnd();
    assert.equal(got, line, `help row ${B.row + r}`);
    // Every block glyph printed green (TERM.ansi[2]).
    let col = 0;
    for (const run of rowRuns(c, row)) {
      for (const ch of run.text) {
        if (col >= B.col && ch !== " ")
          assert.equal(run.color, TERM.ansi[2], `row ${B.row + r} col ${col}`);
        col++;
      }
    }
  });
  // The tagline left of the chef is the name card's.
  assert.equal(rowText(c, last.rows[1]).slice(0, B.col).trim(), TAGLINE);
});

// Motion, as numbers.

test("the open's cells land inside a half-beat cascade and resolve on beat 4", () => {
  const lands = CHEF_CELLS.map(cellLands);
  const first = Math.min(...lands);
  const last = Math.max(...lands);
  assert.ok(
    last - first <= DUR.staggerMax + 1e-9,
    `cascade takes ${last - first} beats`,
  );
  assert.ok(
    last <= OPEN_CUE.resolved,
    "every cell lands before the chef resolves",
  );
  for (const c of CHEF_CELLS) {
    assert.equal(cellReveal(c, OPEN_CUE.reveal - 1e-6), 0);
    assert.equal(cellReveal(c, OPEN_CUE.resolved), 1);
  }
});

test("the bloom settles exactly on the halo, and the morph rests on the end card's chef", () => {
  assert.deepEqual(bloomAt(0), HALO);
  assert.deepEqual(bloomAt(DUR.flick + 2), HALO);
  assert.ok(bloomAt(DUR.flick).alpha > HALO.alpha);
  for (const b of [MORPH_CUE.still, 7.5, 8 - 1 / (120 * BEAT)]) {
    const s = morphState(b);
    assert.equal(s.tint, 0, `tint at ${b}`);
    assert.equal(s.hat, 1);
    assert.equal(s.drop, 1);
    assert.equal(s.discs, 0);
    assert.equal(s.body, 1);
    assert.deepEqual(s.glow, HALO);
  }
  // Nothing has left the card on the downbeat.
  const s0 = morphState(0);
  assert.deepEqual(s0.flight, [0, 0, 0]);
  assert.equal(s0.dip, 0);
  assert.equal(s0.body, 0);
});

// The name card and the end card.

test("the name card's copy is mise's own: Cargo.toml, README.md and the site", () => {
  const cargo = readFileSync(join(REPO, "Cargo.toml"), "utf8");
  assert.equal(/^description = "([^"]+)"/m.exec(cargo)?.[1], TAGLINE);
  const readme = readFileSync(join(REPO, "README.md"), "utf8");
  assert.ok(readme.includes(`<b>${TAGLINE}</b>`), "README.md's tagline");
  assert.ok(readme.includes(INSTALL.command), "README.md's install command");
  const winget = /`([^`]+)`/.exec(PLATFORM)?.[1] ?? "";
  assert.ok(readme.includes(`\`${winget}\``), "README.md's Windows line");
  assert.equal(URL, "mise.jdx.dev");
  assert.match(
    readFileSync(join(REPO, "docs/.vitepress/config.ts"), "utf8"),
    /mise\.jdx\.dev/,
  );
});

test("the end card's cues sit on the reel's eighth-beat grid, mise on the bar line", () => {
  // Each is a whole number of eighths of a beat (0.125 s at 60 BPM) after
  // E0, the card's bar line, so the card moves with the bed's grid.
  for (const [k, t] of Object.entries(END_CUES)) {
    const n = (t * 8) / BEAT;
    assert.ok(
      Math.abs(n - Math.round(n)) < 1e-9,
      `${k} at ${t} is not on an eighth`,
    );
  }
  assert.equal(END_CUES.mise, 0, "mise writes on on the bar line");
  // The card was first keyed to the sung line's onsets (board5.py, seconds
  // after E0); each cue is the eighth nearest its onset, so the motion
  // keeps the feel it was drawn with.
  const onset: [keyof typeof END_CUES, number][] = [
    ["en", 0.36],
    ["place", 0.68],
    ["tagline", 1.48],
    ["install", 2.78],
    ["typed", 5.2],
    ["platform", 6.25],
    ["url", 7.515],
    ["glint", 8.264],
  ];
  for (const [k, at] of onset)
    assert.equal(
      END_CUES[k],
      (Math.round((at * 8) / BEAT) * BEAT) / 8,
      `${k} is not the eighth nearest ${at}`,
    );
  const order = Object.values(END_CUES);
  assert.deepEqual(
    [...order].sort((a, b) => a - b),
    order,
    "cues in order",
  );
  assert.ok(END_CUES.glint + 1 < sec("end").len);
});

test("the end card holds the chef alone on its bar line and is still from 9.5 s", () => {
  const s0 = endCardState(0);
  // Nothing but the chef at rest (=== 0, so an eased -0 counts as none).
  const { taglineFrom, ...rest } = s0;
  assert.equal(taglineFrom, END_CUES.tagline);
  for (const [k, v] of Object.entries({
    ...rest,
    name: s0.name.reduce((a, b) => a + b),
  }))
    if (typeof v === "number")
      assert.ok(v === 0, `${k} is ${v} on the bar line`);
  assert.equal(s0.cursor, false);
  assert.deepEqual(s0.glow, HALO);
  // Each moment starts on its cue.
  const c = END_CUES;
  assert.equal(endCardState(c.en).name[1], 0);
  assert.ok(endCardState(c.en + 0.05).name[1] > 0);
  assert.equal(endCardState(c.install - 1e-6).typed, 0);
  assert.equal(endCardState(c.install).typed, 1);
  assert.equal(endCardState(c.install).box, 1);
  assert.equal(endCardState(c.typed - 1e-3).typed, INSTALL.command.length - 1);
  assert.equal(endCardState(c.typed).typed, INSTALL.command.length);
  assert.equal(endCardState(c.typed).cursor, false);
  // mise.jdx.dev lands on its cue, in over the 1/8 beat before it.
  assert.equal(endCardState(c.url - beats(DUR.flick)).url, 0);
  assert.equal(endCardState(c.url).url, 1);
  assert.equal(endCardState(c.glint).glint, 0);
  // Still from 9.5 s (the glint's sparkle out) to the reel's last frame.
  assert.deepEqual(endCardState(9.5), endCardState(sec("end").len - 1 / 120));
});

// In Chromium.

const load = createRequire(join(REPO, "package.json"));

async function brandPage(t: import("node:test").TestContext) {
  const { chromium } = load(
    "playwright-core",
  ) as typeof import("playwright-core");
  const { build } = load("esbuild") as typeof import("esbuild");
  const { chromiumPath } = (await import(
    pathToFileURL(join(REPO, "docs/.vitepress/showreel-chromium.mjs")).href
  )) as { chromiumPath(): string | undefined };
  let browser: import("playwright-core").Browser;
  try {
    browser = await chromium.launch({ executablePath: chromiumPath() });
  } catch (err) {
    if (process.env.SHOWREEL_REQUIRE_CHROMIUM) throw err;
    t.skip(`no Chromium to draw in: ${String(err).split("\n")[0]}`);
    return null;
  }
  const bundle = await build({
    stdin: {
      contents: `export * from "./kit/chef";
export * as card from "./kit/namecard";
export { CHEF_ART } from "./kit/style";
export { resetTypeCache } from "./type";`,
      resolveDir: SHOWREEL,
      loader: "ts",
    },
    bundle: true,
    format: "iife",
    globalName: "Brand",
    target: "es2022",
    write: false,
    logLevel: "error",
  });
  const fonts = FONTS.map((f) => ({
    ...f,
    bytes: readFileSync(join(REPO, "docs/.vitepress/fonts", f.file)).toString(
      "base64",
    ),
  }));
  const page = await browser.newPage();
  const errors: Error[] = [];
  page.on("pageerror", (err) => errors.push(err));
  await page.setContent("<body></body>");
  await page.addScriptTag({ content: bundle.outputFiles[0].text });
  await page.evaluate(async (fonts) => {
    for (const f of fonts) {
      const bytes = Uint8Array.from(atob(f.bytes), (c) => c.charCodeAt(0));
      document.fonts.add(
        await new FontFace(f.family, bytes, {
          weight: f.weight,
          style: f.style,
        }).load(),
      );
    }
    Brand.resetTypeCache();
  }, fonts);
  return { browser, page, errors };
}

test("in Chromium, the chef is logo-dark.svg, and what the reel does to it stays on it", async (t) => {
  const b = await brandPage(t);
  if (!b) return;
  const { browser, page, errors } = b;
  const svg = readFileSync(join(REPO, "docs/public/logo-dark.svg"), "utf8");
  try {
    await t.test(
      "the settled vector chef matches the SVG as Chromium renders it",
      async () => {
        // At 2 and 3 px a unit, where the viewBox's aspect is whole pixels,
        // on transparent canvases: the SVG's coverage against the chef's.
        const r = await page.evaluate(async (svg) => {
          const V = Brand.CHEF_ART.viewBox;
          const img = new Image();
          img.src = `data:image/svg+xml;base64,${btoa(svg)}`;
          await img.decode();
          return [2, 3].map((k) => {
            const w = V.w * k;
            const h = V.h * k;
            const a = document.createElement("canvas");
            a.width = w;
            a.height = h;
            a.getContext("2d")!.drawImage(img, 0, 0, w, h);
            const c = document.createElement("canvas");
            c.width = w;
            c.height = h;
            Brand.drawChef(
              c.getContext("2d")!,
              { x: 0, y: 0, w, h },
              { shadow: 0 },
            );
            const A = a.getContext("2d")!.getImageData(0, 0, w, h).data;
            const C = c.getContext("2d")!.getImageData(0, 0, w, h).data;
            let max = 0;
            let over = 0;
            let sum = 0;
            let ink = 0;
            for (let i = 3; i < A.length; i += 4) {
              const d = Math.abs(A[i] - C[i]);
              max = Math.max(max, d);
              sum += d;
              if (d > 2) over++;
              if (A[i] > 127) ink++;
            }
            return { k, max, over, mean: sum / (w * h), ink, px: w * h };
          });
        }, svg);
        for (const x of r) {
          t.diagnostic(
            `at ${x.k} px a unit: max ${x.max}, mean ${x.mean.toFixed(4)}, ${x.over} px over 2 of ${x.px}`,
          );
          assert.ok(x.ink > x.px * 0.4, "the SVG drew");
          // Filled path by path, as the browser draws the file, it is the
          // same raster: a level of rounding at most.
          assert.ok(x.max <= 1, `max coverage difference ${x.max} levels`);
          assert.equal(x.over, 0, `${x.over} px differ by more than 2 levels`);
        }
      },
    );

    await t.test(
      "the toque's measured geometry sits on the SVG's own lines",
      async () => {
        const r = await page.evaluate(() => {
          const all = new Path2D();
          for (const d of Brand.CHEF_PATHS) all.addPath(new Path2D(d));
          const ctx = document.createElement("canvas").getContext("2d")!;
          const fill = (x: number, y: number) => ctx.isPointInPath(all, x, y);
          // Each lobe's circle runs down the middle of its dark ring (7 units
          // wide): 2 units either side of it is still ring wherever the ring
          // shows (paper just outside and inside it).
          const lobes = Brand.TOQUE.lobes.map((l) => {
            let on = 0;
            let n = 0;
            for (let a = 0; a < 360; a += 3) {
              const th = (a * Math.PI) / 180;
              // The dark run of a ring's width within 5 units of the circle.
              let s: number | null = null;
              for (let r = l.r - 5; r <= l.r + 5; r += 0.1) {
                const dark = !fill(
                  l.x + r * Math.cos(th),
                  l.y + r * Math.sin(th),
                );
                if (dark && s === null) s = r;
                if (!dark && s !== null) {
                  if (r - s > 5.5 && r - s < 8.5) {
                    n++;
                    if (Math.abs((s + r) / 2 - l.r) <= 1) on++;
                  }
                  break;
                }
              }
            }
            return { on, n };
          });
          // The cut: sampled every half unit, how much of it crosses paper.
          const H = Brand.TOQUE.hat;
          let paper = 0;
          let n = 0;
          for (let i = Brand.TOQUE.cutFrom; i < H.length - 1; i++) {
            const [x0, y0] = H[i];
            const [x1, y1] = H[i + 1];
            const len = Math.hypot(x1 - x0, y1 - y0);
            for (let s = 0; s <= len; s += 0.5) {
              n++;
              if (fill(x0 + ((x1 - x0) * s) / len, y0 + ((y1 - y0) * s) / len))
                paper++;
            }
          }
          // All of the hat's paper within reach of the resolve's circles.
          const hat = new Path2D();
          H.forEach(([x, y], i) => (i ? hat.lineTo(x, y) : hat.moveTo(x, y)));
          hat.closePath();
          let far = 0;
          const V = Brand.CHEF_ART.viewBox;
          for (let x = V.x; x < V.x + V.w; x += 1)
            for (let y = V.y; y < V.y + V.h; y += 1)
              if (ctx.isPointInPath(hat, x, y) && fill(x, y)) {
                const d = Math.min(
                  ...Brand.TOQUE.lobes.map((l) => Math.hypot(x - l.x, y - l.y)),
                );
                if (d > 96) far++;
              }
          return { lobes, paper, n, far };
        });
        r.lobes.forEach((l, i) => {
          t.diagnostic(
            `lobe ${i}: on its ring at ${l.on} of ${l.n} angles where the ring shows`,
          );
          assert.ok(
            l.n >= 40 && l.on >= 0.9 * l.n,
            `lobe ${i}'s circle is off its ring`,
          );
        });
        t.diagnostic(`the cut crosses paper at ${r.paper} of ${r.n} samples`);
        assert.ok(
          r.paper <= 0.2 * r.n,
          "the hat's cut runs through the brim's line",
        );
        assert.equal(r.far, 0, "hat paper beyond the resolve's reach");
      },
    );

    await t.test(
      "the block chef's mosaic lands on the vector chef",
      async () => {
        const r = await page.evaluate(() => {
          const P = Brand.CHEF_ART.place;
          const V = Brand.CHEF_ART.viewBox;
          const s = P.w / V.w;
          const outline = new Path2D(
            Brand.CHEF_PATHS[2].slice(0, Brand.CHEF_PATHS[2].indexOf("M", 1)),
          );
          const hat = new Path2D();
          Brand.TOQUE.hat.forEach(([x, y], i) =>
            i ? hat.lineTo(x, y) : hat.moveTo(x, y),
          );
          hat.closePath();
          const ctx = document.createElement("canvas").getContext("2d")!;
          let inside = 0;
          let hatIn = 0;
          let hatN = 0;
          for (const c of Brand.CHEF_CELLS) {
            const p = Brand.cellInMosaic(c, P);
            const x = (p.cx - P.x) / s + V.x;
            const y = (p.cy - P.y) / s + V.y;
            if (ctx.isPointInPath(outline, x, y)) inside++;
            if (c.row <= 3) {
              hatN++;
              if (ctx.isPointInPath(hat, x, y)) hatIn++;
            }
          }
          return { inside, n: Brand.CHEF_CELLS.length, hatIn, hatN };
        });
        t.diagnostic(
          `${r.inside} of ${r.n} cells land inside the silhouette; ${r.hatIn} of ${r.hatN} hat cells on the hat`,
        );
        assert.ok(r.inside >= 0.9 * r.n);
        assert.ok(r.hatIn >= 0.85 * r.hatN);
      },
    );

    await t.test(
      "the glint and the lobes' tint never leave the chef's silhouette",
      async () => {
        const r = await page.evaluate(() => {
          const V = Brand.CHEF_ART.viewBox;
          const w = V.w * 2;
          const h = V.h * 2;
          const draw = (o: Parameters<typeof Brand.drawChef>[2]) => {
            const c = document.createElement("canvas");
            c.width = w;
            c.height = h;
            Brand.drawChef(
              c.getContext("2d")!,
              { x: 0, y: 0, w, h },
              { shadow: 0, ...o },
            );
            return c.getContext("2d")!.getImageData(0, 0, w, h).data;
          };
          const m = document.createElement("canvas");
          m.width = w;
          m.height = h;
          const mc = m.getContext("2d")!;
          mc.scale(2, 2);
          mc.translate(-V.x, -V.y);
          mc.fill(
            new Path2D(
              Brand.CHEF_PATHS[2].slice(0, Brand.CHEF_PATHS[2].indexOf("M", 1)),
            ),
          );
          const M = mc.getImageData(0, 0, w, h).data;
          const plain = draw({});
          let leaked = 0;
          let lit = 0;
          for (const u of [0.2, 0.5, 0.8]) {
            const g = draw({ glint: u, tint: 1 });
            for (let i = 3; i < g.length; i += 4) {
              if (M[i] === 0 && g[i] !== plain[i]) leaked++;
              if (g[i - 1] !== plain[i - 1] || g[i] !== plain[i]) lit++;
            }
          }
          return { leaked, lit };
        });
        assert.equal(
          r.leaked,
          0,
          `${r.leaked} px changed outside the silhouette`,
        );
        assert.ok(r.lit > 1000, "the glint and tint drew");
      },
    );

    await t.test(
      "every frame is the same whichever frame was drawn before it",
      async () => {
        const r = await page.evaluate(() => {
          const ctx = document
            .createElement("canvas")
            .getContext("2d", { alpha: false })!;
          ctx.canvas.width = 1920;
          ctx.canvas.height = 1080;
          const cell = (i: number, c: number) => ({
            x: 188 + c * 19.2,
            y: 192 + i * 44,
            w: 19.2,
            h: 44,
          });
          const bands = [0, 1, 2].map((i) => ({
            x: 1152,
            y: 190 + 52 * i,
            w: 596,
            h: 38,
          }));
          const frames: [string, () => void][] = [];
          for (const b of [2.55, 3, 3.5, 3.9, 4.3, 6.8])
            frames.push([
              `open ${b}`,
              () => Brand.drawOpenChef(ctx, b, { cell, t: b * 0.8 }),
            ]);
          for (const b of [0.1, 0.9, 3, 4.3, 4.6, 5.5, 7.5])
            frames.push([
              `morph ${b}`,
              () =>
                Brand.drawMorphChef(ctx, b, {
                  bands,
                  labels: ["[tools]", "[env]", "[tasks]"],
                  t: 180 + b * 0.8,
                }),
            ]);
          for (const lt of [0, 0.3, 2.9, 7.6, 8.35, 8.6, 12])
            frames.push([
              `end ${lt}`,
              () => Brand.card.drawEndCard(ctx, lt, 193.6 + lt),
            ]);
          const digest = (fn: () => void) => {
            ctx.setTransform(1, 0, 0, 1, 0, 0);
            ctx.globalAlpha = 1;
            ctx.fillStyle = "#171417";
            ctx.fillRect(0, 0, 1920, 1080);
            fn();
            const d = ctx.getImageData(0, 0, 1920, 1080).data;
            let h = 0x811c9dc5;
            for (let i = 0; i < d.length; i++)
              h = Math.imul(h ^ d[i], 0x01000193);
            return h >>> 0;
          };
          const first = frames.map(([, fn]) => digest(fn));
          const differ: string[] = [];
          [...frames].reverse().forEach(([name, fn], j) => {
            if (digest(fn) !== first[frames.length - 1 - j]) differ.push(name);
          });
          return { differ, n: frames.length };
        });
        t.diagnostic(`${r.n} frames drawn forward and backward`);
        assert.deepEqual(r.differ, []);
      },
    );
    assert.deepEqual(errors, [], "no frame throws");
  } finally {
    await browser.close();
  }
});

// The page's globals, for the type checker.
declare const Brand: typeof import("../kit/chef") & {
  card: typeof import("../kit/namecard");
  CHEF_ART: typeof import("../kit/style").CHEF_ART;
  resetTypeCache: typeof import("../type").resetTypeCache;
};
