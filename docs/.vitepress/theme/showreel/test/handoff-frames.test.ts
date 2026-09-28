// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/handoff-frames.test.ts
// The frames on the bar lines (handoff.ts), drawn. For every hold, the
// outgoing scene's last frame (its end − 1/120 s) and the incoming scene's
// first (its start) must each be drawHandoff's frame at the same global
// time; across the whip, new's first frame must be the handoff's. All of it
// with no facts, with empty facts, and with the capture set's facts when
// there is one: a bar line's persistent layers (kit/rest.ts: the station
// card, the terminal's window and its screen, the chef) are drawn from the
// facts, so both sides must draw the same layers under each set. Frames are
// the reel's raw ones (no vignette, captions or grain), drawn by the reel's
// own compositor with the bundled fonts, so this holds whichever scenes are
// in scenes/index.ts. Last, every frame must come out the same whatever was
// drawn before it, as the renderer's parallel pages need.
//
// A failing frame is written out as got, want and diff PNGs under the
// system temp directory, and the failure names the files. Skipped when no
// Chromium is found (showreel-chromium.mjs: SHOWREEL_CHROMIUM, or
// Playwright's headless shell), unless SHOWREEL_REQUIRE_CHROMIUM is set.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import { DURATION, type ReelFacts } from "../bible";
import type { Reel } from "../compose";
import { FONTS } from "../fonts";
import { BOUNDARIES, HANDOFFS } from "../handoff";
import { loadFacts } from "../load";
import { REPO, SHOWREEL } from "./repo";

/** The last frame before a bar line at 120 fps, the finest render. */
const LAST = 1 / 120;
/** Largest difference any pixel may show, in 8-bit levels of any channel. */
const TOLERANCE = 1;

/** The facts each frame is drawn with: none, an empty set, and the capture set's when there is one. */
const loaded = loadFacts(REPO).facts;
const VARIANTS: Record<string, ReelFacts | null> = {
  none: null,
  empty: {},
  ...(loaded ? { captures: loaded as unknown as ReelFacts } : {}),
};

// From the checkout's root, where the browser and bundler are installed.
// Loaded at run time, not bundled into the test.
const load = createRequire(join(REPO, "package.json"));

/** In the page: the reel's raw frames and the handoffs' frames, and a pixel diff. */
const HELPERS = String.raw`
window.H = {
  canvas() {
    const c = document.createElement("canvas");
    c.width = 1920;
    c.height = 1080;
    return c.getContext("2d", { alpha: false });
  },
  pixels(ctx) {
    return ctx.getImageData(0, 0, 1920, 1080).data;
  },
  // A scene whose every frame is boundary id's: the handoff drawn by the
  // same compositor, so both sides of a comparison go through one path.
  handoffReel(id, facts) {
    const h = Showreel.HANDOFFS[id];
    const scene = { id: h.to, start: 0, end: 1e9, draw: (ctx, lt, env) => Showreel.drawHandoff(ctx, id, env) };
    return Showreel.composeReel([scene], facts, { raw: true });
  },
  frame(reel, t) {
    const ctx = H.canvas();
    reel.render(ctx, t, 1920, 1080);
    return ctx;
  },
  // The largest channel difference, how many pixels differ past the
  // tolerance, and their bounds.
  compare(A, B, tol) {
    let max = 0, over = 0, x0 = 1920, y0 = 1080, x1 = -1, y1 = -1;
    for (let i = 0; i < A.length; i += 4) {
      const d = Math.max(Math.abs(A[i] - B[i]), Math.abs(A[i + 1] - B[i + 1]), Math.abs(A[i + 2] - B[i + 2]));
      if (d > max) max = d;
      if (d > tol) {
        over++;
        const p = i >> 2, x = p % 1920, y = (p / 1920) | 0;
        if (x < x0) x0 = x;
        if (y < y0) y0 = y;
        if (x > x1) x1 = x;
        if (y > y1) y1 = y;
      }
    }
    return { max, over, box: over ? [x0, y0, x1 + 1, y1 + 1] : null };
  },
  // The reference dimmed, with every pixel past the tolerance in red.
  diff(want, A, B, tol) {
    const ctx = H.canvas();
    ctx.drawImage(want.canvas, 0, 0);
    ctx.fillStyle = "rgba(0,0,0,0.6)";
    ctx.fillRect(0, 0, 1920, 1080);
    const img = ctx.getImageData(0, 0, 1920, 1080);
    for (let i = 0; i < A.length; i += 4) {
      const d = Math.max(Math.abs(A[i] - B[i]), Math.abs(A[i + 1] - B[i + 1]), Math.abs(A[i + 2] - B[i + 2]));
      if (d > tol) {
        img.data[i] = 255;
        img.data[i + 1] = 40;
        img.data[i + 2] = 40;
      }
    }
    ctx.putImageData(img, 0, 0);
    return ctx;
  },
};`;

/** The frames one boundary pins: what each is, which side of the bar line it is on, and its global time. */
function checks(
  id: (typeof BOUNDARIES)[number],
): { what: string; side: "out" | "in"; t: number }[] {
  const h = HANDOFFS[id];
  const incoming = {
    what: `${h.to}'s first frame (${h.t.toFixed(3)} s)`,
    side: "in" as const,
    t: h.t,
  };
  // Across the whip only the bar line itself is a fixed frame.
  if (h.meet === "motion") return [incoming];
  return [
    {
      what: `${h.from}'s last frame (${(h.t - LAST).toFixed(3)} s)`,
      side: "out",
      t: h.t - LAST,
    },
    incoming,
  ];
}

test("in Chromium, every scene meets its bar lines on the handoff's frame", async (t) => {
  const { chromium } = load(
    "playwright-core",
  ) as typeof import("playwright-core");
  const { build } = load("esbuild") as typeof import("esbuild");
  const { chromiumPath } = (await import(
    pathToFileURL(join(REPO, "docs/.vitepress/showreel-chromium.mjs")).href
  )) as {
    chromiumPath(): string | undefined;
  };
  let browser: import("playwright-core").Browser;
  try {
    browser = await chromium.launch({ executablePath: chromiumPath() });
  } catch (err) {
    if (process.env.SHOWREEL_REQUIRE_CHROMIUM) throw err;
    t.skip(`no Chromium to draw in: ${String(err).split("\n")[0]}`);
    return;
  }
  try {
    const bundle = await build({
      stdin: {
        contents: `export { composeReel } from "./compose";
export { scenes } from "./scenes";
export { drawHandoff, HANDOFFS } from "./handoff";
export { resetTypeCache } from "./type";`,
        resolveDir: SHOWREEL,
        loader: "ts",
      },
      bundle: true,
      format: "iife",
      globalName: "Showreel",
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
    await page.addScriptTag({ content: HELPERS });
    // Fonts as the renderer loads them, and the layout cache cleared after, so
    // no measurement survives from a fallback face.
    await page.evaluate(
      async ({ fonts, facts }) => {
        for (const f of fonts) {
          const bytes = Uint8Array.from(atob(f.bytes), (c) => c.charCodeAt(0));
          document.fonts.add(
            await new FontFace(f.family, bytes, {
              weight: f.weight,
              style: f.style,
            }).load(),
          );
        }
        Showreel.resetTypeCache();
        const w = window as unknown as {
          reels: typeof reels;
          facts: typeof facts;
        };
        w.reels = Object.fromEntries(
          Object.entries(facts).map(([v, f]) => [
            v,
            Showreel.composeReel(Showreel.scenes, f, { raw: true }),
          ]),
        );
        w.facts = facts;
      },
      { fonts, facts: VARIANTS },
    );
    assert.deepEqual(errors, [], "the reel loads without an error");

    const variants = Object.keys(VARIANTS);
    const out = join(tmpdir(), "showreel-handoffs");
    for (const id of BOUNDARIES) {
      await t.test(id, async () => {
        const failures: string[] = [];
        for (const { what, side, t: at } of checks(id)) {
          const r = await page.evaluate(
            ({ id, at, variants, tol }) => {
              const results: ({
                variant: string;
                kind: "handoff" | "scene";
                png?: string[];
              } & Diff)[] = [];
              let inked = Infinity;
              for (const v of variants) {
                // The handoff under this set of facts: its persistent layers read them.
                const want = H.frame(H.handoffReel(id, facts[v]), at);
                const W = H.pixels(want);
                const got = H.frame(reels[v], at);
                const G = H.pixels(got);
                const c = H.compare(W, G, tol);
                const png =
                  c.max > tol
                    ? [got, want, H.diff(want, W, G, tol)].map((x) =>
                        x.canvas.toDataURL("image/png"),
                      )
                    : undefined;
                results.push({ variant: v, kind: "scene", ...c, png });
                // Pixels unlike the corner's, so a blank frame on both sides cannot pass.
                let n = 0;
                for (let i = 4; i < W.length; i += 4)
                  if (W[i] !== W[0] || W[i + 1] !== W[1] || W[i + 2] !== W[2])
                    n++;
                inked = Math.min(inked, n);
              }
              return { results, inked };
            },
            { id, at, variants, tol: TOLERANCE },
          );
          assert.ok(
            r.inked > 5000,
            `the ${id} handoff at ${at.toFixed(3)} s is nearly blank (${r.inked} px drawn)`,
          );
          for (const x of r.results) {
            t.diagnostic(
              `${id} ${side} [${x.variant}]: max ${x.max} levels, ${x.over} px over ${TOLERANCE}`,
            );
            if (!x.png) continue;
            const dir = join(
              out,
              `${id.replace("|", "-")}_${side}_${x.variant}`,
            );
            mkdirSync(dir, { recursive: true });
            ["got", "want", "diff"].forEach((name, i) =>
              writeFileSync(
                join(dir, `${name}.png`),
                Buffer.from(x.png![i].split(",")[1], "base64"),
              ),
            );
            failures.push(
              `${what} is not the ${id} handoff under the "${x.variant}" facts: ${x.over} px differ by more than ${TOLERANCE} (max ${x.max}) in x ${x.box![0]}–${x.box![2]}, y ${x.box![1]}–${x.box![3]}; see ${dir}/{got,want,diff}.png`,
            );
          }
        }
        assert.deepEqual(errors, [], "no frame throws");
        assert.ok(!failures.length, failures.join("\n"));
      });
    }

    // The renderer draws frames on several pages at once, each in its own
    // order, so a frame that leans on whatever was drawn before it (a cache
    // keyed on the wrong thing, a layer left dirty) flickers in the video.
    await t.test(
      "every frame is the same whichever frame was drawn before it",
      async () => {
        const r = await page.evaluate(
          ({ step, duration }) => {
            const reel = Showreel.composeReel(Showreel.scenes, facts.none, {});
            const ctx = document
              .createElement("canvas")
              .getContext("2d", { alpha: false })!;
            ctx.canvas.width = 960;
            ctx.canvas.height = 540;
            const times: number[] = [];
            for (let t = 0; t < duration; t += step) times.push(t);
            const digest = (t: number) => {
              reel.render(ctx, t, 960, 540);
              const d = ctx.getImageData(0, 0, 960, 540).data;
              let h = 0x811c9dc5;
              for (let i = 0; i < d.length; i++)
                h = Math.imul(h ^ d[i], 0x01000193);
              return h >>> 0;
            };
            const forward = new Map(times.map((t) => [t, digest(t)]));
            const differ: number[] = [];
            for (const t of [...times].reverse())
              if (digest(t) !== forward.get(t)) differ.push(t);
            // Each frame straight after one far away in the reel.
            for (let i = 0; i < times.length; i++) {
              digest(times[(i * 7 + 3) % times.length]);
              if (digest(times[i]) !== forward.get(times[i]))
                differ.push(times[i]);
            }
            return {
              frames: times.length,
              differ: [...new Set(differ)].sort((a, b) => a - b),
            };
          },
          // Off the beat grid, so frames fall mid-motion as well as on rests.
          { step: 0.75, duration: DURATION },
        );
        t.diagnostic(`${r.frames} frames drawn forward, backward and shuffled`);
        assert.deepEqual(errors, [], "no frame throws");
        assert.deepEqual(
          r.differ,
          [],
          `frames that change with the frame drawn before them: ${r.differ.map((x) => `${x} s`).join(", ")}`,
        );
      },
    );
  } finally {
    await browser.close();
  }
});

// The page's globals, for the type checker.
declare const Showreel: typeof import("../compose") &
  typeof import("../handoff") &
  typeof import("../scenes") &
  typeof import("../type");
interface Diff {
  max: number;
  over: number;
  box: number[] | null;
}
declare const H: {
  handoffReel(id: string, facts: ReelFacts | null): Reel;
  frame(reel: Reel, t: number): CanvasRenderingContext2D;
  pixels(ctx: CanvasRenderingContext2D): Uint8ClampedArray;
  compare(a: Uint8ClampedArray, b: Uint8ClampedArray, tol: number): Diff;
  diff(
    want: CanvasRenderingContext2D,
    a: Uint8ClampedArray,
    b: Uint8ClampedArray,
    tol: number,
  ): CanvasRenderingContext2D;
};
declare const reels: Record<string, Reel>;
declare const facts: Record<string, ReelFacts | null>;
