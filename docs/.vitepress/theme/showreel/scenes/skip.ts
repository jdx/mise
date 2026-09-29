// skip (plan v3 §3, Act IV; ART.md §12.2), one thing at a time
// (STORYBOARD.md "Pacing", kit/pace.ts): the card is depends' at rest, and
// becomes C12's as one object (ART.md §7): the build task's table splits
// out of the fold, its header growing in as the fold's pill swaps "2 more
// tables" for "1 more table", and `sources` and `outputs` seat on it (the
// file differs only there). C12 runs `mise run ci` twice; only the second
// run is on camera, from its own prompt (the first and its `Finished in`
// are above it, cropped away), typed at TYPE_RATE while the card waits
// dimmed, and the shot holds its last frame before its own `Finished in`,
// `[build] sources up-to-date, skipping` first, still, for its read. Then,
// as in depends, the window dissolves and the run's lines explode into
// lanes and print in the take's order, not to scale; then build's lane is
// the hero: it grows, its line set large, the others dimmed, and the
// "skipped" stamp lands on it. Caption 1 says why, and leaves once read.
// The lanes leave, and the terminal's window comes back over them with
// args' prompt already in it; the card holds through the bar line, and
// args slides it out as its script card comes in (kit/rest.ts skip|args,
// ART.md §7 "Title change").

import {
  type Capture,
  capture,
  CUT_ON,
  fileOf,
  firstWith,
  lastBefore,
  type ReelData,
  reelData,
  type ScreenOptions,
  stepOf,
} from "../captures";
import { lanes } from "../kit/diagrams/lanes";
import { enter, span } from "../kit/motion";
import {
  beatOf,
  CARD,
  card,
  cut,
  greyScene,
  keep,
  LEFT,
  type Play,
  shot,
  step,
} from "../kit/grey";
import { type FocusPlan, focusOf, Pace } from "../kit/pace";
import { STAMP_LAND } from "../kit/parts";
import { DUR } from "../kit/style";
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

/** The second run from its own prompt: the first run and its `Finished in` are above it, dropped. */
const FROM_PROMPT: ScreenOptions = { from: /^~\/work\/api \$/ };
const TAKE = "ci-skip";
const TITLE = "~/work/api/mise.toml";
const PATH = "work/api/mise.toml";
const TASKS = { "[tasks": 1 } as const;

/**
 * The card's motion, beats after it starts: build's table splits out of
 * the fold and opens (card() grows its header in and swaps the fold's
 * pill); from half way, `sources`'s gap opens and it drops into its slot,
 * `outputs` an eighth later: one motion.
 */
const SEAT_DUR = DUR.seatGap + DUR.seat;
const CARD_MOVE = {
  sources: DUR.fold / 2 + SEAT_DUR,
  outputs: DUR.fold / 2 + SEAT_DUR + DUR.flick,
} as const;

/**
 * The lanes build, beats after it starts: the window dissolves, the rows
 * stand free, then fly (the "not to scale" badge with them) and print in
 * their lanes an eighth apart as they land; then the hero grows (1/2), its
 * stamp starting in as it has all but grown and landing STAMP_LAND later,
 * its ring half a beat from there.
 */
const BUILD = {
  fly: DUR.flick,
  flight: DUR.half,
  print: DUR.flick + DUR.half,
  gap: DUR.flick,
};
const HERO = {
  stamp: DUR.short,
  dur: DUR.short + STAMP_LAND + DUR.half,
};
/**
 * The lanes leave (each falling and fading over DUR.exit, a sixteenth
 * apart) as the window comes back over them (DUR.enter), args' prompt in it.
 */
const BACK_DUR = DUR.enter;

/** The second run's last frame before its `Finished in`. */
const clean = (c: Capture): number => {
  const s = stepOf(c, TAKE);
  return lastBefore(c, CUT_ON, s.enter, s.end);
};

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C12") : null;

interface Beats {
  unfold: number;
  lift: number;
  hero: number;
  leave: number;
}

/**
 * The section's schedule (kit/pace.ts), in the order STORYBOARD.md's event
 * plan gives: one focal motion at a time, half a beat apart; the run's
 * lines held for their read before they leave the pane for the lanes.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("skip", { d, from: 0 });
  const seat = p.move("seat", CARD_MOVE.outputs);
  const ci = p.term("ci", (at) => [step(c, TAKE, at, { until: clean(c) })]);
  p.readTerm();
  const n = flownLines(c, LEFT, {
    after: stepOf(c, TAKE).enter,
    until: clean(c),
    screen: FROM_PROMPT,
    since: 0,
    flies: 0,
    flight: 0,
    print: () => 0,
  }).length;
  const build = p.move(
    "lanes",
    BUILD.print + Math.max(0, n - 1) * BUILD.gap + DUR.flick,
  );
  const hero = p.move("hero", HERO.dur);
  const cap = p.caption(0);
  // It leaves once read (its wipe), and the stage stays still half a beat more.
  p.wait(cap.out + DUR.tick + DUR.half);
  const back = p.move("back", BACK_DUR);
  const lines = flownLines(c, LEFT, {
    after: stepOf(c, TAKE).enter,
    until: clean(c),
    screen: FROM_PROMPT,
    since: build.at,
    flies: build.at + BUILD.fly,
    flight: BUILD.flight,
    print: (k) => build.at + BUILD.print + k * BUILD.gap,
  });
  const plays: Play[] = [cut(0, stepOf(c, TAKE).type), ...ci.plays];
  // The card waits dimmed while the run and the lanes act, and comes back
  // up as the caption names its `sources` and `outputs`.
  const focus: FocusPlan = [
    [ci.at, ["term", "lanes"]],
    [cap.at, "*"],
  ];
  const beats: Beats = {
    unfold: seat.at,
    lift: build.at,
    hero: hero.at,
    leave: back.at,
  };
  return { pace: p, plays, lines, focus, events: p.events(), beats };
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
const FALLBACK: Beats = { unfold: 0, lift: 3.77, hero: 5.77, leave: 12.25 };

/** The window: up from the bar line, dissolving at the lift (glide, no pop), gone while the lanes hold the column, back for args. */
function windowOf(B: Beats, b: number): { alpha: number; dy: number } {
  if (b < B.leave)
    return { alpha: 1 - span(b, B.lift, DUR.exit, "glide"), dy: 0 };
  return enter(b, B.leave);
}

/** A seated line: its drop (0 to 1 over the gap and the landing) and its mark (held a beat, then fading). */
const seat = (b: number, land: number) => ({
  drop: span(b, land - SEAT_DUR, SEAT_DUR, "linear"),
  a:
    (b >= land ? 1 : 0) *
    (1 - span(b, land + DUR.seatHold, DUR.seatFade, "glide")),
});

export const cues: SceneCues<"skip"> = (facts) => {
  const s = schedule(reelData(facts));
  if (!s) return {};
  const B = s.beats;
  // Two clicks: `sources`, then `outputs` an eighth after; the thunk as
  // the stamp lands (STAMP_LAND after it starts in).
  const c = take(reelData(facts))!;
  const at = laneEntrances(s.lines);
  const of = (n: string) => at.get(n) ?? null;
  const hero = firstWith(c, /sources up-to-date/, stepOf(c, TAKE).enter);
  return {
    seat: B.unfold + CARD_MOVE.sources,
    stamp: B.hero + HERO.stamp + STAMP_LAND,
    skipped: hero === null ? null : beatOf(s.plays, hero),
    build: of("build"),
    test: of("test"),
    lint: of("lint"),
    ci: of("ci"),
  };
};

export const scene = greyScene(
  "skip",
  (g) => {
    const { ctx, b } = g;
    const s = schedule(g.d);
    const B = s?.beats ?? FALLBACK;
    const focus: FocusPlan = s?.focus ?? [];
    // The card: C12's file, folded exactly as depends left it (the two
    // files differ only in the build table, which is folded) until the
    // build table splits out of the fold and opens, its two lines seating;
    // it holds through the bar line into args (kit/rest.ts skip|args).
    // (Until the split starts, it is drawn from C11, as the depends|skip
    // rest has it: the same rows, and the same labelled box when the
    // capture set lacks the takes.)
    const from = b <= B.unfold ? "C11" : "C12";
    keep(g, () =>
      card(ctx, CARD, {
        title: TITLE,
        text: fileOf(g.d, from, PATH),
        from,
        open: ["[tasks.ci]"],
        unfold: { "[tasks.build]": span(b, B.unfold, DUR.fold, "move") },
        seat: [
          { re: /^sources = /, ...seat(b, B.unfold + CARD_MOVE.sources) },
          { re: /^outputs = /, ...seat(b, B.unfold + CARD_MOVE.outputs) },
        ],
        lit: TASKS,
        alpha: focusOf(focus, "card", b),
      }),
    );
    const w = windowOf(B, b);
    if (b < B.leave) windowAt(g, LEFT, w.alpha, w.dy);
    // The second run in the pane until the lift, from the prompt the bar
    // line holds (kept: whole on the first frame); then its task lines are
    // the lanes'.
    shot(g, "C12", LEFT, -1, B.lift + DUR.exit, () => s?.plays ?? [], {
      bare: true,
      keep: true,
      fade: DUR.exit,
      screen: FROM_PROMPT,
      lineAlpha: (i, texts) => (b >= B.lift && isTaskLine(texts[i]) ? 0 : 1),
    });
    if (s)
      lanes(ctx, {
        lines: s.lines,
        b,
        badgeAt: B.lift + BUILD.fly,
        hero: { task: "build", at: B.hero, stampAt: B.hero + HERO.stamp },
        leave: B.leave,
      });
    // The window comes back over the leaving lanes, args' prompt in it.
    if (b >= B.leave) returnWindow(g, LEFT, "skip|args", w.alpha, w.dy);
  },
  {
    lit: (b, d) =>
      paneLit(LEFT, windowOf(schedule(d)?.beats ?? FALLBACK, b).alpha),
    events: (d): SceneEvents | null => schedule(d)?.events ?? null,
  },
);
