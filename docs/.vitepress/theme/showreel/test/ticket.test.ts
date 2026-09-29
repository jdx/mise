// The ticket rail and its tickets (kit/ticket.ts, ART.md §10): what each
// act's ticket says, held to the storyboard and the timeline's acts; its
// two beats, as numbers (clear on both bar lines, the printer's steps on the
// grid, at rest while it hangs, clear of the frame when it has lifted); and
// in Chromium, every title set at its size inside the ticket, and the ticket
// drawn as it should be at rest and nowhere on either bar line.
//
// The Chromium test skips when no Chromium is found, unless
// SHOWREEL_REQUIRE_CHROMIUM is set.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import { actOf, BEAT, PILLAR, SECTIONS, type SectionId, sec } from "../bible";
import { FONTS } from "../fonts";
import { DUR, MOTION, TICKET } from "../kit/style";
import {
  PRINT_TICKS,
  ticketFor,
  ticketLift,
  ticketPlace,
  ticketPose,
} from "../kit/ticket";
import { REPO, SHOWREEL } from "./repo";

const TICKETS = SECTIONS.filter((s) => "ticket" in s && s.ticket).map(
  (s) => s.id as SectionId,
);
const C = TICKET.cue;

test("each act's ticket names it: its numeral, its name, its pillar", () => {
  const numerals = ["I", "II", "III", "IV", "V", "VI", "VII"];
  assert.deepEqual(
    TICKETS.map((id) => ticketFor(id).meta),
    numerals.map((n) => `ACT ${n}`),
  );
  for (const id of TICKETS) {
    const t = ticketFor(id);
    const act = actOf(id);
    assert.equal(t.title, act.label, id);
    assert.equal(t.pillar, act.pillar, id);
    assert.equal(t.narrow, id === "new", id);
    // No number on a ticket but its act's numeral.
    assert.doesNotMatch(`${t.title} ${t.chip}`, /\d/, id);
  }
  assert.throws(() => ticketFor("use"), /not a ticket/);
});

test("each ticket's chip is the storyboard's", () => {
  const board = readFileSync(join(SHOWREEL, "STORYBOARD.md"), "utf8");
  for (const id of TICKETS) {
    const t = ticketFor(id);
    const line = board.split("\n").find((l) => l.startsWith(`- **\`${id}\`**`));
    assert.ok(line, `the storyboard has no ${id} section`);
    const m = /[Tt]icket:? "([^"]+)"(?: with `([^`]+)`)?/.exec(line);
    assert.ok(m, `${id}: no ticket in the storyboard's line`);
    assert.equal(m[1], t.title, `${id}'s title`);
    assert.equal(m[2] ?? "", t.chip, `${id}'s chip`);
  }
});

test("the printer steps on the 1/16 grid, a tick each, and stops at the tear", () => {
  assert.equal(PRINT_TICKS.length, TICKET.feedSteps);
  PRINT_TICKS.forEach((b, k) => {
    assert.ok(
      Math.abs(b * 16 - Math.round(b * 16)) < 1e-9,
      `step ${k} at ${b}`,
    );
    if (k) assert.ok(b > PRINT_TICKS[k - 1]);
  });
  assert.ok(PRINT_TICKS[PRINT_TICKS.length - 1] < C.tear);
  // The paper only ever moves down while it feeds, one step at a time.
  let last = -Infinity;
  for (let b = 0.001; b < C.tear; b += 0.005) {
    const p = ticketPose(b)!;
    assert.ok(p.dy >= last - 1e-9, `the feed went back up at ${b}`);
    last = p.dy;
  }
});

test("nothing shows on either bar line; the ticket hangs still before it lifts", () => {
  for (const narrow of [false, true]) {
    const { rail, paper } = ticketPlace(narrow);
    assert.equal(ticketPose(0, { narrow }), null, "beat 0");
    assert.equal(ticketPose(C.liftEnd, { narrow }), null, "the lift's end");
    assert.equal(
      ticketPose(2 - 1 / (120 * BEAT), { narrow }),
      null,
      "the last frame",
    );
    // At rest: on its hanging place, square, under a rail at rest.
    for (const b of [1.1, 1.15, C.liftAt]) {
      const p = ticketPose(b, { narrow })!;
      assert.ok(Math.abs(p.dy) < 1e-9, `dy ${p.dy} at ${b}`);
      assert.ok(Math.abs(p.rot) < 1e-9, `rot ${p.rot} at ${b}`);
      assert.equal(p.railY, rail.y);
      assert.equal(p.alpha, 1);
      assert.ok(p.torn);
    }
    // It comes out from under the rail (its foot never above the rail's),
    // and once torn it hangs wholly below the rail's middle, where the
    // drawing cuts the paper still inside the rail.
    for (let b = 0.01; b < C.liftAt; b += 0.01) {
      const p = ticketPose(b, { narrow })!;
      assert.ok(
        paper.y + paper.h + p.dy >= p.railY + rail.h - 1e-9,
        `the foot above the rail at ${b}`,
      );
      if (b >= C.tear)
        assert.ok(
          paper.y + p.dy >= p.railY + rail.h / 2,
          `paper above the rail at ${b}`,
        );
    }
    // The dip before the lift: down, opposite the move.
    const dip = ticketPose(C.liftAt + DUR.anticipate, { narrow })!;
    assert.ok(Math.abs(dip.dy - MOTION.anticipate) < 1e-9);
    // Clear of the frame's top, shadow and all, as it fades out.
    const late = ticketPose(C.liftEnd - 1e-4, { narrow });
    if (late)
      assert.ok(paper.y + paper.h + late.dy < -50, "still on the frame");
  }
  assert.equal(ticketLift(C.liftAt), 0);
  assert.equal(ticketLift(C.liftEnd), 1);
});

test("a ticket is a pure function of its beat", () => {
  for (let b = 0; b <= 2; b += 0.037)
    assert.deepEqual(ticketPose(b), ticketPose(b), `at ${b}`);
  assert.ok(sec("tools").ticket && sec("tools").beats === 2);
});

// In Chromium.

const load = createRequire(join(REPO, "package.json"));

test("in Chromium, every ticket sets its title and chip inside it, and draws nothing on its bar lines", async (t) => {
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
    return;
  }
  try {
    const bundle = await build({
      stdin: {
        contents: `export * from "./kit/ticket";
export { TICKET, TYPE } from "./kit/style";
export { layout, serifItalic, font, MONO, resetTypeCache } from "./type";`,
        resolveDir: SHOWREEL,
        loader: "ts",
      },
      bundle: true,
      format: "iife",
      globalName: "Tix",
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
      Tix.resetTypeCache();
    }, fonts);
    const r = await page.evaluate((ids) => {
      const ctx = document
        .createElement("canvas")
        .getContext("2d", { alpha: false })!;
      ctx.canvas.width = 1920;
      ctx.canvas.height = 1080;
      const T = Tix.TICKET;
      const Y = Tix.TYPE.ticket;
      const out = ids.map((id) => {
        const spec = Tix.ticketFor(id as never);
        const paper = spec.narrow ? T.narrow : T.ticket;
        const room = Tix.titleRoom(paper.w);
        const size = spec.narrow ? Y.titleNarrow : Y.title;
        const full = Tix.layout(
          ctx,
          spec.title,
          Tix.serifItalic(size, Y.titleWeight),
        ).width;
        const set = Tix.titleFont(ctx, spec.title, size, room);
        const title = Tix.layout(ctx, spec.title, set).width;
        const shrunk = full > room;
        const chip = Tix.layout(
          ctx,
          spec.chip,
          Tix.font(Y.band, Y.bandWeight, Tix.MONO),
        ).width;
        // Drawn at rest: the band's colour in the band, paper beside the title.
        const blank = () => {
          ctx.setTransform(1, 0, 0, 1, 0, 0);
          ctx.fillStyle = "#171417";
          ctx.fillRect(0, 0, 1920, 1080);
        };
        const at = (x: number, y: number) =>
          Array.from(ctx.getImageData(x, y, 1, 1).data.slice(0, 3));
        blank();
        Tix.drawTicket(ctx, 1.15, spec);
        const band = at(
          paper.x + T.band.inset + 4,
          paper.y + T.band.y + T.band.h / 2,
        );
        const sheet = at(paper.x + 10, paper.y + T.titleY + 30);
        // Nothing on the bar lines, nor from the lift's end.
        const drawnAt = (b: number) => {
          blank();
          Tix.drawTicket(ctx, b, spec);
          const d = ctx.getImageData(0, 0, 1920, 1080).data;
          for (let i = 0; i < d.length; i += 4)
            if (d[i] !== 0x17 || d[i + 1] !== 0x14 || d[i + 2] !== 0x17)
              return true;
          return false;
        };
        return {
          id,
          title,
          set,
          shrunk,
          chip,
          room,
          band,
          sheet,
          at0: drawnAt(0),
          at18: drawnAt(T.cue.liftEnd),
          at2: drawnAt(2 - 1 / 96),
        };
      });
      return out;
    }, TICKETS);
    assert.deepEqual(errors, [], "no ticket throws");
    const hex = (c: number[]) =>
      `#${c.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
    for (const x of r) {
      t.diagnostic(
        `${x.id}: title ${x.title.toFixed(0)} px (${x.set}), chip ${x.chip.toFixed(0)} px, of ${x.room}`,
      );
      // Every title fits; only the narrow ticket's steps down from its size.
      assert.ok(x.title <= x.room, `${x.id}'s title is wider than its ticket`);
      assert.equal(x.shrunk, x.id === "new", `${x.id} set at ${x.set}`);
      assert.ok(x.chip <= x.room, `${x.id}'s chip is wider than its band`);
      assert.equal(
        hex(x.band),
        PILLAR[ticketFor(x.id).pillar],
        `${x.id}'s band`,
      );
      // Ticket paper, between its top and bottom tones.
      const [rr, gg, bb] = x.sheet;
      assert.ok(
        rr > 225 && gg > 220 && bb > 205,
        `${x.id}'s paper is ${hex(x.sheet)}`,
      );
      assert.ok(!x.at0 && !x.at18 && !x.at2, `${x.id} draws on a bar line`);
    }
  } finally {
    await browser.close();
  }
});

// The page's globals, for the type checker.
declare const Tix: typeof import("../kit/ticket") &
  typeof import("../type") & {
    TICKET: typeof import("../kit/style").TICKET;
    TYPE: typeof import("../kit/style").TYPE;
  };
