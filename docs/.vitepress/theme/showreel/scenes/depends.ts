// depends (plan v3 §3, Act IV; ART.md §12.2), one thing at a time
// (STORYBOARD.md "Pacing", kit/pace.ts): `mise run ci` (C11) types at the
// prompt the Tasks ticket brought in, at TYPE_RATE, and prints at real
// time: every task's lines, prefixed, in the order they came, while the
// card waits dimmed. Once the output has held its read in the pane (2 s,
// rule 3), the window dissolves round it and its task lines explode into
// lanes, sorting themselves by task: each prefix to the label column, the
// rest to its task's row. They land
// queued, dimmed, and print in the order the take printed them, not to
// scale (the badge comes with them): the three dependencies together, an
// eighth apart, so they read as running in parallel, then, a clear half
// beat later, `ci`, whose lane waited in a dashed outline, and the bracket
// ties them to it. The lines hold there, still, while caption 1 says it;
// then the card comes back up and marks its `depends` line, the labels
// underline, and caption 2 says each line is labeled. The lanes hold on,
// still, until caption 2 has been read; then they settle into the card's
// task headers: each strip
// gathers its lines in, shrinks to a band and flies into its header's
// pillar bar as the header lights. Then the terminal's window comes back
// with skip's own prompt in it (never empty), and the card holds through
// both bar lines.

import { BEAT } from "../bible";
import {
  type Capture,
  capture,
  CUT_ON,
  fileOf,
  lastBefore,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import { lanes, settleHandover } from "../kit/diagrams/lanes";
import { bump, enter, span } from "../kit/motion";
import {
  CARD,
  card,
  type CardRows,
  cut,
  greyScene,
  keep,
  LEFT,
  type Play,
  shot,
  step,
} from "../kit/grey";
import { type FocusPlan, focusOf, Pace } from "../kit/pace";
import { OPENS } from "../kit/rest";
import { CARD_ART, DUR, PILLAR_BAND, TYPE } from "../kit/style";
import type { SceneCues } from "../score/cues";
import type { SceneEvents } from "../storyboard";
import {
  flownLines,
  isTaskLine,
  laneEntrances,
  paneLit,
  returnWindow,
  windowAt,
} from "./g4-tasks/common";

/** ci's table, open on the card; the others fold under the first of theirs. */
const CI_HEADER = "[tasks.ci]";

/**
 * The lanes build, in beats after it starts (one focal motion): the window
 * dissolves round the output and its task lines stand free, then fly into
 * their lanes (the "not to scale" badge with them); the dependencies'
 * lines print an eighth apart in the take's order, so the three strips
 * grow together (they run in parallel); ci's lines, which wait for them, a
 * clear half beat after, a quarter apart, and the bracket ties them.
 */
const BUILD = {
  fly: DUR.tick,
  flight: DUR.move,
  print: DUR.tick + DUR.move,
  depGap: DUR.flick,
  ci: DUR.tick + DUR.move + DUR.half,
  ciGap: DUR.tick,
} as const;
const BUILD_DUR = 2;
/** The lanes settle into the card's task headers, each strip over DUR.move, a sixteenth apart. */
const SETTLE_DUR = DUR.move;

/** The run's last frame before `Finished in`. */
const clean = (c: Capture): number => {
  const ci = stepOf(c, "ci");
  return lastBefore(c, CUT_ON, ci.type, ci.end);
};

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C11") : null;

/** The section's beats: when each motion starts, and where the lanes land. */
interface Beats {
  lift: number;
  mark: number;
  labels: number;
  settle: number;
  back: number;
}

/**
 * The section's schedule (kit/pace.ts), in the order STORYBOARD.md's event
 * plan gives, one focal motion at a time: `mise run ci` typed at TYPE_RATE;
 * its output held its read in the pane (2 s: the lanes carry its lines on
 * to be read), then its task lines explode into lanes, where they hold,
 * still, through both captions; the card marks the `depends` line once
 * caption 1 has said it; the labels underline before caption 2 says each
 * line is labeled; once caption 2 has been read the lanes settle into the
 * card; the terminal comes back for skip.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("depends", { d });
  const ci = p.term("ci", (at) => [step(c, "ci", at, { until: clean(c) })]);
  p.readTerm();
  const build = p.move("lanes", BUILD_DUR);
  p.caption(0);
  const mark = p.move("mark", DUR.tick);
  const labels = p.move("labels", DUR.move);
  // Caption 2 names the lanes' labels: they hold until it has been read.
  p.wait(p.caption(1).out);
  const lines = flownLines(c, LEFT, {
    after: stepOf(c, "ci").type,
    until: clean(c),
    since: build.at,
    flies: build.at + BUILD.fly,
    flight: BUILD.flight,
    print: (k, _text, all) => {
      const root = all.findIndex((t) => /^\[ci\] /.test(t));
      return root < 0 || k < root
        ? build.at + BUILD.print + k * BUILD.depGap
        : build.at + BUILD.ci + (k - root) * BUILD.ciGap;
    },
  });
  const n = laneEntrances(lines).size;
  const settle = p.move(
    "settle",
    Math.max(0, n - 1) * DUR.stagger + SETTLE_DUR,
  );
  const back = p.move("skipPrompt", DUR.enter);
  const plays: Play[] = [cut(0, OPENS.C11!(c)), ...ci.plays];
  const focus: FocusPlan = [
    [ci.at, ["term", "lanes"]],
    [mark.at, "*"],
  ];
  const beats: Beats = {
    lift: build.at,
    mark: mark.at,
    labels: labels.at,
    settle: settle.at,
    back: back.at,
  };
  return { pace: p, plays, focus, lines, events: p.events(), beats };
}

type Plan = ReturnType<typeof plan>;
const PLANS = new WeakMap<Capture, Plan>();
/** The schedule for a take (a pure function of it and the facts' captions); null without it. */
export function schedule(d: ReelData | null): Plan | null {
  const c = take(d);
  if (!c) return null;
  let s = PLANS.get(c);
  if (!s) PLANS.set(c, (s = plan(d, c)));
  return s;
}

/** Without the take: the storyboard's event plan (sections.json). */
const FALLBACK: Beats = {
  lift: 4.03,
  mark: 8.75,
  labels: 9.5,
  settle: 16.5,
  back: 17.94,
};

/** score/depends.ts plays air from 0.3 s before the settle cue to 0.3 s after, and the click there. */
const SCORE_AIR = 0.3 / BEAT;

/** When lane `i` (in the take's order) hands over to its header's band. */
const landed = (B: Beats, i: number): number =>
  settleHandover(B.settle, i, SETTLE_DUR);

/** The window: up until the lift (dissolving on glide, no pop), gone while the lanes hold the column, back for skip. */
function windowOf(B: Beats, b: number): { alpha: number; dy: number } {
  if (b < B.back)
    return { alpha: 1 - span(b, B.lift, DUR.exit, "glide"), dy: 0 };
  return enter(b, B.back);
}

export const cues: SceneCues<"depends"> = (facts) => {
  const s = schedule(reelData(facts));
  if (!s) return {};
  const at = laneEntrances(s.lines);
  const of = (n: string) => at.get(n) ?? null;
  return {
    build: of("build"),
    lint: of("lint"),
    test: of("test"),
    ci: of("ci"),
    // The air under the first lane's move, and its click as its strip
    // becomes the header's band.
    settle: landed(s.beats, 0) - SCORE_AIR,
  };
};

export const scene = greyScene(
  "depends",
  (g) => {
    const { ctx, b } = g;
    const s = schedule(g.d);
    const B = s?.beats ?? FALLBACK;
    const focus: FocusPlan = s?.focus ?? [];
    // The card: `depends` marked once caption 1 has said it; the task
    // headers light as the lanes land in them: the dependencies' folded
    // header as the first lands, ci's as ci's does.
    const order = s ? [...laneEntrances(s.lines).keys()] : [];
    const ciAt = landed(B, Math.max(0, order.indexOf("ci")));
    const depAt = landed(
      B,
      Math.max(
        0,
        order.findIndex((n) => n !== "ci"),
      ),
    );
    const text = fileOf(g.d, "C11", "work/api/mise.toml");
    // The dependencies' tables fold together under the first one's header.
    const deps =
      text
        ?.split("\n")
        .find((l) => /^\[tasks\./.test(l) && !l.startsWith(CI_HEADER)) ??
      "[tasks.";
    const mark =
      span(b, B.mark, DUR.tick, "arrive") *
      (1 - span(b, B.labels, DUR.half, "glide"));
    const rows: CardRows = keep(g, () =>
      card(ctx, CARD, {
        title: "~/work/api/mise.toml",
        text,
        from: "C11",
        open: [CI_HEADER],
        seat: [{ re: /^depends = /, a: mark }],
        lit: {
          [deps]: span(b, depAt, DUR.light, "arrive"),
          [CI_HEADER]: span(b, ciAt, DUR.light, "arrive"),
        },
        ignite: {
          [deps]: bump(b, depAt, DUR.ignite),
          [CI_HEADER]: bump(b, ciAt, DUR.ignite),
        },
        alpha: focusOf(focus, "card", b),
      }),
    );
    const w = windowOf(B, b);
    if (b < B.back) windowAt(g, LEFT, w.alpha, w.dy);
    else returnWindow(g, LEFT, "depends|skip", w.alpha, w.dy);
    // The run in the pane until the lift; from then its task lines are the lanes'.
    shot(g, "C11", LEFT, -1, B.lift + DUR.exit, () => s?.plays ?? [], {
      bare: true,
      keep: true,
      fade: DUR.exit,
      lineAlpha: (i, texts) => (b >= B.lift && isTaskLine(texts[i]) ? 0 : 1),
    });
    if (!s) return;
    // Where each lane settles: the band (ART.md §7) of the folded
    // dependencies' header, or of ci's own, which then lights.
    const header = (name: string) => {
      const r = rows.find((x) => x.kind === "header" && x.text === name);
      const band = {
        x: CARD.x + CARD_ART.bandInset,
        w: CARD.w - 2 * CARD_ART.bandInset,
      };
      return r && r.top !== undefined && r.h !== undefined
        ? { ...band, y: r.top, h: r.h }
        : { ...band, y: CARD.y, h: TYPE.card.lineH };
    };
    lanes(ctx, {
      lines: s.lines,
      b,
      badgeAt: B.lift + BUILD.fly,
      labels: { at: B.labels, until: B.settle },
      settle: {
        at: B.settle,
        dur: SETTLE_DUR,
        to: (name) => header(name === "ci" ? CI_HEADER : deps),
        band: PILLAR_BAND.tasks,
      },
    });
  },
  {
    lit: (b, d) =>
      paneLit(LEFT, windowOf(schedule(d)?.beats ?? FALLBACK, b).alpha),
    events: (d): SceneEvents | null => schedule(d)?.events ?? null,
  },
);
