// args (plan v3 §3, Act IV; ART.md §12.15), one thing at a time
// (STORYBOARD.md "Pacing", kit/pace.ts): skip's card slides out right and
// the deploy script's file card slides in beside the terminal (a title
// change), then unfolds its `#USAGE` block, lit. `mise run deploy --help`
// types at TYPE_RATE and prints its help: its top, a viewport crop of the
// command line and seven rows, down to the allowed values (its two Flags
// rows stay below the crop), held for its read while caption 1 says what
// it shows. The help gives way to the next command's own prompt, and
// `mise run deploy prod` prints four real `mise ERROR` lines (C13). Half a
// beat on, a paper outline takes `prod` in the error line and the pane's
// text shakes, side to side like a head saying no, and settles; a thread
// ties `prod` to the `choices` line the script declares, out of the
// outline's top and along the empty end of the row above (never over the
// error line's own words), and caption 2 says it, leaving once read. Then
// the marks and the card go and the text dissolves into daemons' own
// prompt, which the window holds into daemons (kit/rest.ts args|daemons
// keeps no card). The card dims while the terminal acts.

import { BEAT } from "../bible";
import {
  type Capture,
  capture,
  firstWith,
  fixture,
  keyAt,
  type ReelData,
  reelData,
  type ScreenOptions,
  stepOf,
} from "../captures";
import { roundedRect } from "../fx";
import { clamp, TAU } from "../math";
import { enter, exit, span } from "../kit/motion";
import {
  beatOf,
  CARD,
  cut,
  findRow,
  greyScene,
  keep,
  LEFT,
  type Play,
  rest,
  restScreen,
  shot,
  step,
  threadAlong,
} from "../kit/grey";
import { type FocusPlan, focusOf, Pace, unfocus } from "../kit/pace";
import { drawPaneWindow } from "../kit/rest";
import { COLOR, DUR, MOTION, RADIUS, STROKE, THREAD } from "../kit/style";
import type { SceneCues } from "../score/cues";
import type { SceneEvents } from "../storyboard";
import { paneLit } from "./g4-tasks/common";
import { scriptCard, SCRIPT_TYPE } from "./g4-tasks/script-card";

/**
 * The title change, beats after it starts: skip's card, held through the
 * bar line, slides out; the script card slides in once it has gone (the
 * script card is shorter, so an overlap showed the old card's rows under
 * it).
 */
const SLIDE = { cardIn: DUR.exit, dur: DUR.exit + DUR.enter } as const;
/** The `#USAGE` block unfolds, then lights. */
const USAGE = { light: DUR.fold, dur: DUR.fold + DUR.half } as const;
/**
 * The shake after the error has printed (ART.md §12.15, MOTION.shake):
 * side to side, decaying with `decay` seconds from `amp` px, 0 again from
 * five decays on, like a head shaken no.
 */
const SHAKE_DUR = (5 * MOTION.shake.decay) / BEAT;
/** The thread from `prod` to `choices`, and the seat mark as its head lands. */
const TIE_DUR = DUR.thread + DUR.tick;
/** Clearing the stage for daemons: the marks go, then the card leaves as the text dissolves into daemons' prompt. */
const OUT = { card: DUR.tick, dur: DUR.tick + DUR.enter } as const;

/** The help's top: the command line and seven rows, down to the allowed values. */
const HELP_ROWS = 8;
/** The script's `#USAGE` block, and the line the tie lands on. */
const USAGE_RE = /^#USAGE/;
const CHOICES = /^#USAGE\s+choices /;
const PROD = /Invalid choice for arg \S+: (\S+?),/;

/** The bad run from its own prompt: the help printed above it is cropped away. */
const firstKey = (c: Capture): number => keyAt(c, "m", stepOf(c, "bad").start);
const badScreen =
  (c: Capture) =>
  (ct: number): ScreenOptions =>
    ct < firstKey(c)
      ? { from: /^~\/work\/api \$\s*$/ }
      : { from: /^~\/work\/api \$ m/ };

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C13") : null;

interface Beats {
  slide: number;
  usage: number;
  /** The help gives way to the bad run's own prompt. */
  swap: number;
  shake: number;
  tie: number;
  out: number;
}

/**
 * The section's schedule (kit/pace.ts), in the order STORYBOARD.md's event
 * plan gives, one focal motion at a time: each command typed at TYPE_RATE;
 * the help held for its five rows' read before the bad run's prompt
 * replaces it; the errors held while the shake and the tie mark them.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("args", { d, from: 0 });
  const slide = p.move("slide", SLIDE.dur);
  const usage = p.move("usage", USAGE.dur);
  const help = p.term("help", (at) => [step(c, "help", at)], { lines: 5 });
  const cap1 = p.caption(0);
  // The help holds, whole, for its read: the next prompt replaces it after.
  p.readTerm();
  const swap = p.next();
  const bad = p.term("bad", (at) => [step(c, "bad", at)], {
    lines: 2,
    after: swap + DUR.exit,
  });
  const shake = p.move("shake", SHAKE_DUR);
  const tie = p.move("tie", TIE_DUR);
  const cap = p.caption(1);
  // It leaves once read (its wipe), and the stage stays still half a beat more.
  p.wait(cap.out + DUR.tick + DUR.half);
  const out = p.move("out", OUT.dur);
  const helpPlays: Play[] = [cut(0, stepOf(c, "help").type), ...help.plays];
  const badPlays: Play[] = [cut(swap, stepOf(c, "bad").type), ...bad.plays];
  // The card dims while the terminal acts, and is back up for each
  // caption that names it (the tie brings it back for caption 2).
  const focus: FocusPlan = [
    [help.at, "term"],
    [cap1.at, "*"],
    [bad.at, "term"],
    [tie.at, "*"],
  ];
  const beats: Beats = {
    slide: slide.at,
    usage: usage.at,
    swap,
    shake: shake.at,
    tie: tie.at,
    out: out.at,
  };
  return { pace: p, helpPlays, badPlays, focus, events: p.events(), beats };
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
  slide: 0,
  usage: 1.25,
  swap: 8,
  shake: 11.18,
  tie: 12.18,
  out: 18.75,
};

/** The shake from beat `at`: side to side, decaying, exactly 0 at `at` and from five decays on. */
const SHAKE_HZ = 8;
function shakeAt(b: number, at: number): readonly [number, number] {
  const d = (b - at) * BEAT;
  const { amp, decay } = MOTION.shake;
  if (d <= 0 || d >= 5 * decay) return [0, 0];
  return [amp * Math.exp(-d / decay) * Math.sin(TAU * SHAKE_HZ * d), 0];
}

export const cues: SceneCues<"args"> = (facts) => {
  const s = schedule(reelData(facts));
  if (!s) return {};
  const c = take(reelData(facts))!;
  const B = s.beats;
  const help = firstWith(c, /^Usage:/, stepOf(c, "help").enter);
  return {
    usage: B.usage + USAGE.light,
    help: help === null ? null : beatOf(s.helpPlays, help),
    // The dissonant stab goes with the shake.
    error: B.shake,
    // The soft click as the thread lands on `choices`.
    tie: B.tie + DUR.thread,
  };
};

export const scene = greyScene(
  "args",
  (g) => {
    const { ctx, b } = g;
    const sc = schedule(g.d);
    const B = sc?.beats ?? FALLBACK;
    const focus: FocusPlan = sc?.focus ?? [];
    const script = fixture(g.d, "api/mise-tasks/deploy");
    const lines = script === null ? null : script.trim().split("\n");
    // The card: in from the right, out to the right.
    const cardIn = B.slide + SLIDE.cardIn;
    const cardOut = B.out + OUT.card;
    const clear = cardOut;
    const cin = enter(b, cardIn);
    const cout = exit(b, cardOut);
    const slide =
      MOTION.slideIn * (1 - span(b, cardIn, DUR.enter, "arrive")) +
      MOTION.slideIn * span(b, cardOut, DUR.exit, "leave");
    // skip's card, held through the bar line, slides out right as the
    // script card comes in: a title change (ART.md §7), never an empty
    // column across the bar line.
    // It is gone as the script card starts in: no overlap, and no empty
    // column between them.
    ctx.save();
    ctx.translate(MOTION.slideIn * span(b, B.slide, DUR.exit, "leave"), 0);
    rest(g, "skip|args", 1 - span(b, B.slide, DUR.exit, "glide"), [0]);
    ctx.restore();
    const tie = span(b, B.tie, DUR.thread, "arrive");
    // The marks go first as the stage clears for daemons.
    const tieA = 1 - span(b, B.out, DUR.tick, "glide");
    // The block lights once open, and its light goes as the help gives way.
    const block =
      span(b, B.usage + USAGE.light, DUR.tick, "arrive") *
      (1 - span(b, B.swap, DUR.half, "glide"));
    // The `choices` row takes a seat mark as the thread's head reaches it.
    const landed = span(b, B.tie + DUR.thread - DUR.flick, DUR.tick, "arrive");
    ctx.save();
    ctx.translate(slide, 0);
    const rows = scriptCard(ctx, CARD, {
      title: "mise-tasks/deploy",
      lines,
      from: "fixtures",
      fold: USAGE_RE,
      b,
      unfoldAt: B.usage,
      block,
      hl: [{ re: CHOICES, a: landed * tieA }],
      alpha: cin.alpha * cout.alpha * focusOf(focus, "card", b),
    });
    ctx.restore();
    keep(g, () => drawPaneWindow(ctx, LEFT, unfocus(focus, "term", b)));
    // daemons' prompt comes up as the bad run's text goes (the args|daemons rest).
    restScreen(g, "args|daemons", span(b, clear, DUR.enter, "glide"));
    // The help, cropped to its top, from the prompt the bar line holds
    // (kept: whole on the first frame), until the bad run's prompt replaces it.
    shot(g, "C13", LEFT, -1, B.swap + DUR.exit, () => sc?.helpPlays ?? [], {
      bare: true,
      keep: true,
      fade: DUR.exit,
      screen: { top: HELP_ROWS },
    });
    // The bad run, from its own prompt; its text shakes once it has printed.
    const c = take(g.d);
    const s = shot(
      g,
      "C13",
      LEFT,
      B.swap,
      clear + DUR.enter,
      () => sc?.badPlays ?? [],
      {
        bare: true,
        fade: DUR.exit,
        screen: c ? badScreen(c) : undefined,
        shake: (x) => shakeAt(x, B.shake),
      },
    );
    if (!s || b < B.shake) return;
    const err = findRow(s.screen, PROD);
    const m = err ? PROD.exec(err.text) : null;
    if (!err || !m) return;
    // `prod`, outlined from the impact, shaking with its line.
    const col = err.text.indexOf(m[1], m.index);
    const len = Array.from(m[1]).length;
    const cell = s.layout.cell(err.line, col);
    const size = s.pane.size;
    const base = s.layout.baseline(err.line);
    const inset = 4;
    const box = {
      x: cell.x - inset,
      y: base - 0.8 * size - inset,
      w: len * s.layout.advance + 2 * inset,
      h: size + 2 * inset,
    };
    const pop = span(b, B.shake, DUR.tick, "arrive");
    const boxA = clamp(pop * 2) * tieA;
    if (boxA > 0) {
      ctx.save();
      ctx.globalAlpha *= boxA;
      const sc = 0.9 + 0.1 * pop;
      ctx.translate(box.x + box.w / 2, box.y + box.h / 2);
      ctx.scale(sc, sc);
      roundedRect(ctx, -box.w / 2, -box.h / 2, box.w, box.h, RADIUS.chip - 4);
      ctx.strokeStyle = COLOR.mark;
      ctx.lineWidth = STROKE.lit;
      ctx.stroke();
      ctx.restore();
    }
    // The thread: out of the outline's top edge near its right end, up
    // into the row above past that row's end (the error line's own words
    // run on right of `prod`, so it never rides just over them), along it
    // to the gutter and up to the `choices` row, into the card's left
    // edge. If the row above reaches past `prod`, along the gap between
    // the rows instead.
    const to = rows.find((r) => CHOICES.test(r.text));
    if (!to || tie <= 0 || tieA <= 0) return;
    const rowMid = (to.top ?? to.y) + SCRIPT_TYPE.lineH / 2;
    const above = err.line > 0 ? s.screen.lines[err.line - 1] : null;
    const aboveEnd =
      above === null
        ? -Infinity
        : s.layout.col(Array.from(above.map((r) => r.text).join("")).length);
    const x0 = box.x + box.w - 12;
    const route =
      aboveEnd + 16 <= x0
        ? [
            { x: x0, y: box.y },
            { x: x0, y: base - s.pane.lineH - 0.3 * size },
            { x: THREAD.gutterX, y: base - s.pane.lineH - 0.3 * size },
          ]
        : [
            {
              x: box.x + box.w,
              y: base - 0.8 * size - (s.pane.lineH - size) / 2,
            },
            {
              x: THREAD.gutterX,
              y: base - 0.8 * size - (s.pane.lineH - size) / 2,
            },
          ];
    threadAlong(
      ctx,
      [
        ...route,
        { x: THREAD.gutterX, y: rowMid },
        { x: CARD.x + 8, y: rowMid },
      ],
      tie,
      tieA,
    );
  },
  {
    lit: () => paneLit(LEFT, 1),
    events: (d): SceneEvents | null => schedule(d)?.events ?? null,
  },
);
