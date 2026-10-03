// The landing-page films' edits (edit.ts) and joins (film.ts): every cut
// plays a span of one section at its source pace, the partial shots end
// where the storyboard's events say the source hands over to a neighbour
// the film does not keep, a ticket that follows a section the source did
// not put before it takes that section's stage down, every other cut the
// source did not make leaves and enters on the kit's curves, and the
// tickets are numbered in the film's order.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { BEAT } from "../bible";
import {
  continuous,
  cutAt,
  cuts,
  type Edition,
  END_TO,
  filmChapters,
  filmChaptersVtt,
  filmDuration,
  OPEN_TO,
  SWITCH_TO,
} from "../edit";
import { createFilm, filmJoins, framePresence } from "../film";
import { handoffOut } from "../handoff";
import { setInkSink } from "../kit/ink";
import { END_CUES } from "../kit/namecard";
import { GLINT } from "../kit/chef";
import { DUR } from "../kit/style";
import { loadFacts } from "../load";
import type { Reel } from "../compose";
import { createReel } from "../reel";
import { sec } from "../timeline";
import { recordingContext } from "./record";
import { REPO, SHOWREEL } from "./repo";
import type { ReelFacts } from "../bible";

const EDITIONS = ["tour", "overview"] satisfies Edition[];

interface PlanEvent {
  at: number;
  end: number;
  name: string;
}
const board: { id: string; plan: PlanEvent[] }[] = JSON.parse(
  readFileSync(join(SHOWREEL, "sections.json"), "utf8"),
);
const event = (id: string, name: string): PlanEvent => {
  const e = board.find((s) => s.id === id)?.plan.find((p) => p.name === name);
  if (!e) throw new Error(`${id} has no event ${name}`);
  return e;
};

for (const edition of EDITIONS) {
  test(`${edition}: contiguous cuts, each a span of one section at its source pace`, () => {
    const list = cuts(edition);
    let end = 0;
    for (const cut of list) {
      assert.equal(cut.start, end);
      assert.equal(cut.end - cut.start, cut.to - cut.from);
      const source = sec(cut.id);
      assert.ok(cut.from >= source.start && cut.to <= source.end);
      assert.ok(cut.to > cut.from);
      assert.equal(cutAt(edition, cut.start).id, cut.id);
      assert.equal(cutAt(edition, cut.end - 1 / 120).id, cut.id);
      end = cut.end;
    }
    assert.equal(end, filmDuration(edition));
    assert.equal(cutAt(edition, end).id, "end");
    // Whole sections but for the three the films cut inside.
    for (const cut of list) {
      const s = sec(cut.id);
      if (cut.id === "open" || cut.id === "switch")
        assert.equal(cut.from, s.start);
      else if (cut.id === "end") assert.equal(cut.from, s.start);
      else {
        assert.equal(cut.from, s.start);
        assert.equal(
          cut.to,
          s.end,
          `${cut.id} keeps its complete reading hold`,
        );
      }
    }
  });
  test(`${edition}: chapters cover the delivered film, including reordered scenes`, () => {
    let end = 0;
    const chapters = filmChapters(edition);
    for (const chapter of chapters) {
      assert.equal(chapter.start, end);
      assert.ok(chapter.end > chapter.start);
      end = chapter.end;
    }
    assert.equal(end, filmDuration(edition));
    assert.equal(
      filmChaptersVtt(edition).trimEnd().split("\n\n").length,
      chapters.length + 1,
    );
    assert.equal(new Set(chapters.map((c) => c.id)).size, chapters.length);
    assert.equal(chapters[0].label, "mise");
    assert.equal(chapters.at(-1)!.label, "Install mise");
  });
  test(`${edition}: a ticket after a cut inherits the stage before it; other cuts enter and leave`, () => {
    const list = cuts(edition);
    const joins = filmJoins(edition);
    let acts = 0;
    list.forEach((cut, k) => {
      const prev = list[k - 1];
      const s = sec(cut.id);
      if (s.ticket) {
        assert.equal(
          joins[cut.id]?.meta,
          `ACT ${["I", "II", "III", "IV", "V", "VI", "VII"][acts++]}`,
        );
      } else assert.equal(joins[cut.id], undefined);
      const jump = prev && !continuous(prev, cut);
      if (s.ticket && jump) {
        // The bar line the film's predecessor ends on, which its last frame holds.
        assert.equal(joins[cut.id]?.in, handoffOut(prev.id)?.id);
        assert.equal(prev.to, sec(prev.id).end);
      } else assert.equal(joins[cut.id]?.in, undefined);
      const first = framePresence(list, k, cut.start, joins);
      const mid = framePresence(list, k, (cut.start + cut.end) / 2, joins);
      const last = framePresence(list, k, cut.end - 1 / 120, joins);
      assert.deepEqual(
        mid,
        { alpha: 1, dy: 0 },
        `${cut.id} stands on its mark`,
      );
      if (jump && !joins[cut.id]?.in) {
        assert.equal(first.alpha, 0, `${cut.id} enters`);
        assert.ok(first.dy > 0);
        const landed = framePresence(
          list,
          k,
          cut.start + DUR.enter * BEAT,
          joins,
        );
        assert.deepEqual(landed, { alpha: 1, dy: 0 });
      } else
        assert.deepEqual(
          first,
          { alpha: 1, dy: 0 },
          `${cut.id} arrives at rest`,
        );
      const next = list[k + 1];
      if (next && !continuous(cut, next) && !joins[next.id]?.in) {
        assert.ok(last.alpha < 0.1, `${cut.id} leaves`);
        assert.ok(last.dy > 0);
        const still = framePresence(list, k, cut.end - DUR.exit * BEAT, joins);
        assert.deepEqual(still, { alpha: 1, dy: 0 });
      } else
        assert.deepEqual(last, { alpha: 1, dy: 0 }, `${cut.id} ends at rest`);
    });
  });
}

test("the partial shots end where the storyboard hands over to a scene the films cut", () => {
  // The open: once its name card and chef have left, before the pitch's terminal rises.
  assert.ok(OPEN_TO >= event("open", "leave").end);
  assert.ok(OPEN_TO <= event("open", "pitchTerm").at);
  // The switch: on the fold into packslip's card, after its threads have drawn.
  assert.equal(SWITCH_TO, event("switch", "fold").at);
  assert.ok(SWITCH_TO >= event("switch", "threads").end + DUR.half);
  // The end card: its last move over (the glint and its sparkle), and a hold.
  assert.ok(END_TO >= END_CUES.glint + GLINT.dur + GLINT.sparkle + 1);
  assert.ok(END_TO <= sec("end").len);
});

test("the tour is under five minutes and the overview is 60–90 seconds, project switching first", () => {
  assert.ok(filmDuration("tour") < 300);
  assert.ok(filmDuration("overview") >= 60 && filmDuration("overview") <= 90);
  assert.equal(cuts("tour")[0].id, "open");
  assert.equal(
    cuts("tour")[1].id,
    "switch",
    "project switching is the first demonstration",
  );
  assert.deepEqual(
    cuts("tour")
      .slice(-2)
      .map((c) => c.id),
    ["morph", "end"],
  );
  assert.equal(cuts("overview").at(-1)!.id, "end");
});

/** Every string a reel draws on one frame, through the kit's ink tap. */
function inked(reel: Reel, t: number): string {
  const g = globalThis as { document?: unknown };
  const had = g.document;
  g.document = {
    createElement: () => ({
      width: 0,
      height: 0,
      getContext: () => recordingContext(() => {}),
    }),
  };
  const out: string[] = [];
  const prev = setInkSink((text) => out.push(text));
  try {
    reel.render(
      recordingContext((text) => out.push(text)),
      t,
      1920,
      1080,
    );
  } finally {
    setInkSink(prev);
    g.document = had;
  }
  return out.join("\n");
}

test("the Environments ticket takes down the registry's card, not packslip's stage", (t) => {
  const { facts } = loadFacts(REPO);
  if (!facts) return t.skip("needs the capture set");
  const f = facts as unknown as ReelFacts;
  const env = cuts("tour").find((c) => c.id === "env")!;
  const registry = cuts("tour").find((c) => c.id === "registry")!;
  assert.equal(registry.end, env.start);
  // Just into the ticket: the inherited stage is still up, leaving.
  const film = inked(createFilm(f, "tour"), env.start + 0.1);
  assert.match(film, /jq = "latest"/, "the registry's card is on the frame");
  assert.doesNotMatch(film, /hk = "/, "packslip's card is not");
  assert.doesNotMatch(
    film,
    /linked ~\/work\/api/,
    "packslip's terminal is not",
  );
  assert.match(film, /ACT II/, "the ticket is numbered in film order");
  // The source reel's own env ticket still takes packslip's stage down.
  const source = inked(createReel(f), sec("env").start + 0.1);
  assert.match(source, /hk = "/);
  assert.match(source, /ACT III/);
});
