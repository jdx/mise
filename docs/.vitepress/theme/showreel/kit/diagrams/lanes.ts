// Lanes (depends, skip, clone; ART.md §12.2): a task run's output exploded
// into one strip of terminal per task, in the order the take printed them
// and in its own ANSI colours, so the colours stay "inside a pane". Each
// lane's captured prefix (`[build]`) stands in the label column; its lines,
// verbatim after the prefix, fill the strip as they print. The task the
// others feed (`ci`) waits in a dashed outline, indented, until its first
// line prints; then a bracket in the gutter ties its dependencies to it.
// A badge says what the picture is: "Order from one real run. Not to scale."
//
// A line can come from a terminal: given where its row stands in the pane
// (`from`), the pane's rows explode into the lanes together, each prefix to
// the label column and the rest to its task's row, sorting the run by task.
// They land queued, dimmed as a pane is, and then print in the order the
// take printed them, each lane's strip growing under its first line. The
// lines are the captured runs throughout; only where they stand changes.
//
// skip: one task's lane grows, its line set large, the others dimmed, and a
// "skipped" stamp lands at its right end. The labels can underline (the
// caption saying each line is labeled). The lanes can settle into the
// card's task headers, or leave. Settling, each strip stays one opaque
// object: it gathers its lines in as it shrinks, in place, to its header
// band's size, then flies to the header's left edge, narrowing into the
// header's pillar bar, and hands over there as the card lights the header
// (its bar, then its band sweeping out from it). It never lies over the
// card's text. A pure function of the lines (each with the beat it
// printed) and the section's beat.

import { PALETTE, PILLAR, TERM } from "../../bible";
import { mix, rgba } from "../../color";
import { roundedRect } from "../../fx";
import { clamp, lerp, progress } from "../../math";
import { font, layout } from "../../type";
import { type Capture, type ScreenOptions, screenAt } from "../../captures";
import { arc, curveAt, exit, lerpRect, type Rect } from "../motion";
import { stamp } from "../parts";
import { advance, type Run } from "../term";
import {
  DASH,
  DUR,
  EASE,
  LANES_ART,
  MOTION,
  PANE_DIM,
  PILLAR_BAND,
  RADIUS,
  SHADOW,
  STROKE,
  TYPE,
  UNDERLINE,
} from "../style";
import {
  badge,
  bumpOver,
  castShadow,
  mono,
  monoBaseline,
  monoW,
  rectPath,
  runsText,
  span,
  splitAt,
  staggerOf,
} from "./common";

/** The badge every lanes picture carries. */
export const LANES_BADGE = "Order from one real run. Not to scale.";

/**
 * Where a line stood in its terminal: column 0's left edge, its baseline
 * and size, and the pane's right-edge fade. From beat `since` the lanes
 * draw it there (the scene stops drawing it in its pane); from `flies` it
 * flies into its lane and lands there, queued, on `lands`, then prints on
 * its own `at`.
 */
export interface LaneSource {
  x: number;
  y: number;
  size: number;
  fade?: readonly [number, number] | null;
  since: number;
  flies: number;
  lands: number;
}

/** A task's output line and the beat it printed in its lane. */
export interface LaneLine {
  runs: readonly Run[];
  at: number;
  /** The terminal row it flies from, landing queued before `at`. */
  from?: LaneSource;
}

/** One line in its lane: the runs after the prefix, the whole row, and where it came from. */
interface LaneRow {
  runs: Run[];
  at: number;
  full: readonly Run[];
  /** The prefix's length, with its space: where `runs` start in `full`. */
  cut: number;
  from?: LaneSource;
}

/** One task's lane: its prefix as captured, its lines after it, and when it first printed. */
export interface Lane {
  name: string;
  label: Run[];
  lines: LaneRow[];
  first: number;
  /** The task the others feed. */
  root: boolean;
}

const PREFIX = /^\[([^\]\s]+)\] ?/;

/**
 * The lanes of a run's task lines, in the order each task first printed:
 * `[name] …` lines grouped by name, each split into its captured prefix
 * (trimmed) and the rest (verbatim, less the one space after the prefix).
 * Lines without a prefix are not a task's, and are left out.
 */
export function laneModel(lines: readonly LaneLine[], root = "ci"): Lane[] {
  const out: Lane[] = [];
  for (const l of lines) {
    const text = runsText(l.runs);
    const m = PREFIX.exec(text);
    if (!m) continue;
    const [label, rest] = splitAt(l.runs, m[0].length);
    let lane = out.find((x) => x.name === m[1]);
    if (!lane) {
      const [pre] = splitAt(label, m[0].trimEnd().length);
      lane = {
        name: m[1],
        label: pre,
        lines: [],
        first: l.at,
        root: m[1] === root,
      };
      out.push(lane);
    }
    lane.lines.push({
      runs: rest,
      at: l.at,
      full: l.runs,
      cut: m[0].length,
      from: l.from,
    });
    lane.first = Math.min(lane.first, l.at);
  }
  return out;
}

/**
 * A take's task lines up to take time `until` (the screen as ScreenOptions
 * crops it), each with the take time it first appeared after `after`: for
 * a scene to map onto its beats and hand to `lanes`. `index` is each
 * line's place in that screen's lines (a pane's row, for `from`).
 */
export function taskLines(
  c: Capture,
  o: { after: number; until: number; screen?: ScreenOptions },
): { runs: Run[]; t: number; index: number }[] {
  const tasks = (t: number) =>
    screenAt(c, t, o.screen ?? {})
      .lines.map((runs, index) => ({ runs, index }))
      .filter((l) => PREFIX.test(runsText(l.runs)));
  const final = tasks(o.until);
  const when = final.map(() => o.after);
  let seen = tasks(o.after).length;
  for (const f of c.frames) {
    if (f.t <= o.after || f.t > o.until + 1e-9) continue;
    const n = Math.min(final.length, tasks(f.t).length);
    for (let k = seen; k < n; k++) when[k] = f.t;
    seen = Math.max(seen, n);
  }
  return final.map((l, k) => ({ runs: l.runs, t: when[k], index: l.index }));
}

export interface LanesOptions {
  lines: readonly LaneLine[];
  /** Local beats. */
  b: number;
  alpha?: number;
  /** The task the others feed (default `ci`). */
  root?: string;
  /** When the "not to scale" badge pops in; none without. */
  badgeAt?: number;
  /** skip: this task's lane grows and is stamped "skipped". */
  hero?: { task: string; at: number; stampAt: number };
  /**
   * Settle into the card from beat `at`: each lane shrinks to its header's
   * band and flies into the header's pillar bar (one header for all, or
   * each lane's own), `dur` beats each (default DUR.move), a sixteenth
   * apart. `to` is the header's band: the row's box from the card's band
   * inset (x + 12, ART.md §7), whose left edge is where the bar stands.
   */
  settle?: {
    at: number;
    to: Rect | ((lane: string) => Rect);
    dur?: number;
    /** The band's colour, which the strips take on as they shrink (default the tasks band). */
    band?: string;
    /** The pillar bar's colour, which they take on as they narrow into it (default tasks). */
    bar?: string;
  };
  /** Leave from beat `at`: each lane falls and fades (DUR.exit), a sixteenth apart. */
  leave?: number;
  /**
   * Underline each lane's label from beat `at` (a cascade, a sixteenth
   * apart), gone by `until`: the caption saying each line is labeled.
   */
  labels?: { at: number; until: number };
}

/**
 * When settling lane `i` (in the order the lanes appear) starts handing
 * over to its header: the beat a scene lights that header from (DUR.light),
 * an eighth before the strip reaches the header's bar, so the card's bar
 * is growing under the strip as it arrives and the band sweeps out from it.
 */
export const settleHandover = (
  at: number,
  i: number,
  dur: number = DUR.move,
): number => at + i * DUR.stagger + SETTLE_ARRIVE * dur - DUR.flick;

/** Where a lane stands on a frame. */
export interface LaneBox {
  name: string;
  rect: Rect;
  mid: number;
}

/** The bracket's corner radius, px. */
const ELBOW = 16;
/**
 * The hero lane (skip): its line's baseline under the lane's top, and its
 * stamp's clearance from the lane's bottom, so the stamp lands in the lower
 * right clear of the line's descenders (nothing is drawn over lane text).
 */
const HERO_BASELINE = 56;
/** 8: at 14 the stamp's 1.3× approach crossed the line's descenders; at 8 it clears them by 14 px. */
const HERO_STAMP_BOTTOM = 8;
/**
 * A lane's later lines carry its prefix too; theirs fade over the first
 * fifth of the flight, before the two prefixes converge, so each lane has
 * one label in the air (at a third, the second showed doubled under the
 * first for a few frames).
 */
const MERGE = 0.2;
/**
 * Settling, as fractions of each strip's span: it shrinks in place to the
 * band's size (its lines gathered in and faded) until SETTLE_SHRINK, flies
 * into the header's bar until SETTLE_ARRIVE, then fades there over the
 * card's own bar as the header lights.
 */
const SETTLE_SHRINK = 0.3;
const SETTLE_ARRIVE = 0.85;
/** The flight's arc: a gentle lift, so the strip rides over the gutter into the bar. */
const SETTLE_LIFT = 0.12;
/** A flown line waits in its lane, before it prints, dimmed as a pane is. */
const QUEUED = PANE_DIM.text;
/** Where a lane's lines start inside its strip. */
const TEXT_INSET = 20;

/** A lane's label baseline and its lines' baselines, as drawLane sets them. */
function laneGeometry(l: Lane, r: Rect, hero: number) {
  const n = l.lines.length;
  const labelMid = lerp(
    r.y + r.h / 2,
    r.y + HERO_BASELINE - 0.36 * TYPE.lane.hero,
    hero,
  );
  return {
    labelY: monoBaseline(labelMid - r.h / 2, r.h, TYPE.lane.label),
    lineY: (j: number) =>
      lerp(
        n === 1
          ? monoBaseline(r.y, LANES_ART.laneH, TYPE.lane.text)
          : r.y + LANES_ART.textY + j * LANES_ART.textPitch,
        r.y + HERO_BASELINE,
        hero,
      ),
    fade: [r.x + r.w - 60, r.x + r.w - 8] as const,
  };
}

/**
 * The lanes on beat `b`. Returns where each lane stands (its track's rect),
 * for a scene that ties something to one.
 */
export function lanes(
  ctx: CanvasRenderingContext2D,
  o: LanesOptions,
): LaneBox[] {
  const L = LANES_ART;
  const model = laneModel(o.lines, o.root ?? "ci");
  const alpha = o.alpha ?? 1;
  const b = o.b;
  // The hero grows (move 1/2) and the others dim to 55 % with it.
  const heroK = o.hero ? span(b, o.hero.at, DUR.half, "move") : 0;
  const heightOf = (l: Lane) =>
    o.hero && l.name === o.hero.task ? lerp(L.laneH, L.heroH, heroK) : L.laneH;
  const firstAny = Math.min(...model.map((l) => l.first));
  const boxes: LaneBox[] = [];
  let y = L.top;
  for (const l of model) {
    const h = heightOf(l);
    const x0 = l.root ? L.trackX0 + L.ciIndent : L.trackX0;
    boxes.push({
      name: l.name,
      rect: { x: x0, y, w: L.trackX1 - x0, h },
      mid: y + h / 2,
    });
    y += h + L.gap;
  }
  if (alpha <= 0 || !model.length) return boxes;
  const bottom = y - L.gap;
  const root = model.findIndex((l) => l.root);
  const rootFirst = root >= 0 ? model[root].first : Infinity;
  // Leaving: each lane falls and fades, a sixteenth apart; the bracket and
  // the badge go with the first.
  const gone = (i: number) =>
    o.leave === undefined
      ? { alpha: 1, dy: 0 }
      : exit(b, o.leave + staggerOf(i, model.length), DUR.exit);
  ctx.save();
  ctx.globalAlpha *= alpha;
  // The bracket and the badge give way as the lanes settle.
  // (Alpha on glide, as exit() fades, so it never ends in a pop.)
  const settling = o.settle ? span(b, o.settle.at, DUR.tick, "glide") : 0;
  const g0 = gone(0);
  if (root > 0) {
    ctx.save();
    ctx.globalAlpha *= g0.alpha;
    ctx.translate(0, g0.dy);
    // The bracket recedes with the lanes the hero dims.
    ctx.globalAlpha *= 1 - 0.45 * heroK;
    drawBracket(
      ctx,
      boxes,
      root,
      span(b, rootFirst, DUR.half, "arrive") * (1 - settling),
    );
    ctx.restore();
  }
  if (o.badgeAt !== undefined) {
    const gb = gone(model.length - 1);
    ctx.save();
    ctx.globalAlpha *= gb.alpha;
    ctx.translate(0, gb.dy);
    badge(ctx, L.trackX1, Math.max(L.badgeY, bottom + 16), LANES_BADGE, {
      align: "right",
      k: span(b, o.badgeAt, DUR.badge, "arrive") * (1 - settling),
    });
    ctx.restore();
  }
  model.forEach((l, i) => {
    const box = boxes[i];
    const isHero = !!o.hero && l.name === o.hero.task;
    const dim = o.hero && !isHero ? 1 - 0.45 * heroK : 1;
    // Settling: each strip shrinks in place to its header's band, gathering
    // its lines in, then flies into the header's pillar bar (a sixteenth
    // apart), and hands over there as the header lights.
    const sd = o.settle?.dur ?? DUR.move;
    const s0 = o.settle ? o.settle.at + i * DUR.stagger : Infinity;
    const su = o.settle ? progress(s0, s0 + sd, b) : 0;
    const gl = gone(i);
    if (su >= 1 || gl.alpha <= 0) return;
    ctx.save();
    ctx.globalAlpha *= dim * gl.alpha;
    ctx.translate(0, gl.dy);
    if (su > 0 && o.settle) {
      const to =
        typeof o.settle.to === "function" ? o.settle.to(l.name) : o.settle.to;
      settleStrip(
        ctx,
        l,
        box,
        b,
        su,
        to,
        o.settle,
        l.root ? firstAny : l.first,
      );
      ctx.restore();
      return;
    }
    drawLane(ctx, l, box, b, l.root ? firstAny : l.first, isHero ? heroK : 0);
    if (o.labels) {
      const k = span(
        b,
        o.labels.at + staggerOf(i, model.length),
        DUR.half,
        "arrive",
      );
      const out = 1 - span(b, o.labels.until - DUR.exit, DUR.exit, "glide");
      // In the bracket's ink (2 px paper at 60 %), drawn on left to right.
      if (k > 0 && out > 0) {
        const B = L.bracket;
        ctx.save();
        ctx.globalAlpha *= out;
        ctx.fillStyle = rgba(B.color, B.alpha);
        ctx.fillRect(
          L.labelX,
          laneGeometry(l, box.rect, isHero ? heroK : 0).labelY + UNDERLINE.dy,
          monoW(runsText(l.label), TYPE.lane.label) * k,
          STROKE.tree,
        );
        ctx.restore();
      }
    }
    if (isHero && o.hero) {
      // The kit's stamp (ART.md §9): in from 1.3× over DUR.stamp from
      // `stampAt`, landing (flash and ring) at stampAt + STAMP_LAND.
      // Clipped to the lane under its line's descenders: the landing's
      // ring (which grows from the stamp's edge) ripples through the
      // lane's lower part, never through the hero line's words or into
      // the next lane.
      const r = box.rect;
      const f = font(TYPE.stamp.size, TYPE.stamp.weight);
      const w = layout(ctx, "skipped", f).width + 40;
      const h = Math.round(TYPE.stamp.size * 1.6);
      const under = r.y + HERO_BASELINE + 0.25 * TYPE.lane.hero;
      ctx.save();
      ctx.beginPath();
      roundedRect(ctx, r.x, under, r.w, r.y + r.h - under, RADIUS.lane);
      ctx.clip();
      stamp(
        ctx,
        {
          x: r.x + r.w - 28 - w,
          y: r.y + r.h - HERO_STAMP_BOTTOM - h,
          w,
          h,
        },
        b - o.hero.stampAt,
        { label: "skipped" },
      );
      ctx.restore();
    }
    ctx.restore();
  });
  // Flown lines in the air, over the lanes.
  model.forEach((l, i) =>
    l.lines.forEach((line, j) => drawFlight(ctx, l, line, j, boxes[i], b)),
  );
  ctx.restore();
  return boxes;
}

/**
 * A flown line out of its pane: at its row from `since`, then in the air
 * from `flies` to `lands`, on an arc (travel)
 * from its terminal row into its lane, its prefix to the label column at
 * the label's size and the rest to its row in the strip, dimming to
 * QUEUED as it goes. A lane's first prefix lands on its label; a later
 * one fades out on the way.
 */
function drawFlight(
  ctx: CanvasRenderingContext2D,
  l: Lane,
  line: LaneRow,
  j: number,
  box: LaneBox,
  b: number,
): void {
  const src = line.from;
  if (!src || b < src.since || b >= src.lands) return;
  const u = progress(src.flies, src.lands, b);
  const k = EASE.travel(u);
  const geo = laneGeometry(l, box.rect, 0);
  const [pre] = splitAt(line.full, line.cut);
  const [label] = splitAt(pre, runsText(pre).trimEnd().length);
  const at = (
    a: { x: number; y: number },
    z: { x: number; y: number },
  ): { x: number; y: number } => curveAt(arc(a, z, MOTION.arcLift), k);
  const size = (s: number) => lerp(src.size, s, k);
  // The pane's right-edge fade eases into the lane's.
  const f = src.fade ?? geo.fade;
  const fade = [
    lerp(f[0], geo.fade[0], k),
    lerp(f[1], geo.fade[1], k),
  ] as const;
  ctx.save();
  ctx.globalAlpha *= lerp(1, QUEUED, k);
  const lp = at({ x: src.x, y: src.y }, { x: LANES_ART.labelX, y: geo.labelY });
  const merge = j === 0 ? 1 : 1 - progress(0, MERGE, u);
  if (merge > 0) {
    ctx.save();
    ctx.globalAlpha *= merge;
    mono(ctx, label, lp.x, lp.y, size(TYPE.lane.label));
    ctx.restore();
  }
  const rp = at(
    { x: src.x + line.cut * advance(src.size), y: src.y },
    { x: box.rect.x + TEXT_INSET, y: geo.lineY(j) },
  );
  mono(ctx, line.runs, rp.x, rp.y, size(TYPE.lane.text), { fade });
  ctx.restore();
}

/**
 * A settling lane `u` of the way through its span (ART.md §12.2):
 *
 * 1. Until SETTLE_SHRINK its strip shrinks in place (move) to the header
 *    band's size, left edge kept, centred on the lane, its fill taking the
 *    band's colour and its shadow lifting to `float`; its lines, clipped to
 *    the shrinking strip, and its label fade where they stand.
 * 2. Until SETTLE_ARRIVE it flies (travel, on a low arc) to the band's left
 *    edge, narrowing into the header's pillar bar (6 px, the band's height)
 *    and taking the bar's colour. Its right edge only ever moves right,
 *    from the lane to the bar's, so it never lies over the card's text.
 * 3. Then it fades there (glide) over the card's own bar, which the scene
 *    lit from settleHandover: the strip becomes the header's light.
 *
 * Opaque throughout: nothing translucent smears across the card.
 */
function settleStrip(
  ctx: CanvasRenderingContext2D,
  l: Lane,
  box: LaneBox,
  b: number,
  u: number,
  to: Rect,
  s: { band?: string; bar?: string },
  appear: number,
): void {
  const r0 = box.rect;
  const bandCol = s.band ?? PILLAR_BAND.tasks;
  const barCol = s.bar ?? PILLAR.tasks;
  const p1 = EASE.move(progress(0, SETTLE_SHRINK, u));
  const p2 = EASE.travel(progress(SETTLE_SHRINK, SETTLE_ARRIVE, u));
  const handed = EASE.glide(progress(SETTLE_ARRIVE, 1, u));
  const band: Rect = {
    x: r0.x,
    y: r0.y + (r0.h - to.h) / 2,
    w: Math.min(to.w, r0.w),
    h: to.h,
  };
  const bar: Rect = { x: to.x, y: to.y, w: STROKE.headerBar, h: to.h };
  let r = lerpRect(r0, band, p1);
  if (p2 > 0) {
    const c = curveAt(
      arc(
        { x: band.x + band.w / 2, y: band.y + band.h / 2 },
        { x: bar.x + bar.w / 2, y: bar.y + bar.h / 2 },
        SETTLE_LIFT,
      ),
      p2,
    );
    const w = lerp(band.w, bar.w, p2);
    const h = lerp(band.h, bar.h, p2);
    r = { x: c.x - w / 2, y: c.y - h / 2, w, h };
  }
  if (handed < 1) {
    ctx.save();
    ctx.globalAlpha *= 1 - handed;
    const air = p1 * (1 - p2);
    const path = rectPath(ctx, r, lerp(RADIUS.lane, 0, Math.max(p1, p2)));
    castShadow(ctx, SHADOW.small, path, 1 - air);
    castShadow(ctx, SHADOW.float, path, air);
    path();
    ctx.fillStyle = mix(TERM.window, bandCol, p1);
    ctx.fill();
    // It flies as a lit header does (ART.md §7): the band, and the pillar
    // bar down its left edge, growing from its middle as it shrinks; the
    // band narrows away in flight until only the bar is left.
    const bh = r.h * p1;
    ctx.fillStyle = barCol;
    ctx.fillRect(
      r.x,
      r.y + (r.h - bh) / 2,
      Math.min(STROKE.headerBar, r.w),
      bh,
    );
    ctx.restore();
  }
  // Its lines, gathered in by the strip (clipped to it), and its label
  // fade where they stand, gone before it flies.
  const text = 1 - progress(0, 0.8 * SETTLE_SHRINK, u);
  if (text > 0) {
    ctx.save();
    ctx.globalAlpha *= text;
    drawLane(ctx, l, box, b, appear, 0, { strip: false, clip: r });
    ctx.restore();
  }
}

/** One lane: its label, its strip (dashed while it waits), and its lines as they print. */
function drawLane(
  ctx: CanvasRenderingContext2D,
  l: Lane,
  box: LaneBox,
  b: number,
  appear: number,
  hero: number,
  o: { strip?: boolean; clip?: Rect } = {},
): void {
  const strip = o.strip ?? true;
  const L = LANES_ART;
  const r = box.rect;
  const flown = l.lines[0]?.from;
  // A flown lane's label and lines stand queued from its first landing.
  const queued = flown ? b >= flown.lands : false;
  // The strip grows from the label rightward as the lane appears.
  const grow = span(b, appear, DUR.half, "arrive");
  if (grow <= 0 && !queued) return;
  // The root waits in a dashed outline until its first line prints.
  const filled = l.root ? span(b, l.first, DUR.tick, "arrive") : 1;
  const w = r.w * grow;
  const labelSize = TYPE.lane.label;
  const geo = laneGeometry(l, r, hero);
  ctx.save();
  // The label: its captured prefix, in its own colour, centred on the lane
  // (dimmed, never recoloured, until its lane prints: the root's until its
  // own first line). A hero's label follows its line up to the lane's
  // upper part.
  const labelA = flown
    ? queued
      ? lerp(QUEUED, 1, l.root ? filled : span(b, l.first, DUR.tick, "arrive"))
      : 0
    : clamp(grow * 1.5) * lerp(0.45, 1, filled);
  if (labelA > 0) {
    ctx.save();
    ctx.globalAlpha *= labelA;
    mono(ctx, l.label, L.labelX, geo.labelY, labelSize);
    ctx.restore();
  }
  if (grow > 0 && strip) {
    ctx.save();
    ctx.globalAlpha *= clamp(grow * 1.5);
    if (filled < 1) {
      ctx.save();
      ctx.globalAlpha *= 1 - filled;
      ctx.setLineDash([...DASH.waiting]);
      ctx.lineWidth = STROKE.tree;
      ctx.strokeStyle = L.waitingStroke;
      roundedRect(ctx, r.x + 1, r.y + 1, w - 2, r.h - 2, RADIUS.lane);
      ctx.stroke();
      ctx.restore();
    }
    if (filled > 0) {
      ctx.save();
      ctx.globalAlpha *= filled;
      const strip = { x: r.x, y: r.y, w, h: r.h };
      const path = rectPath(ctx, strip, RADIUS.lane);
      castShadow(ctx, SHADOW.small, path);
      // A lane receiving a line brightens a little toward paper.
      const flash = Math.max(
        0,
        ...l.lines.map((x) => bumpOver(b, x.at, DUR.half)),
      );
      path();
      ctx.fillStyle = mix(TERM.window, PALETTE.paper, L.flash * flash);
      ctx.fill();
      roundedRect(ctx, r.x + 0.5, r.y + 0.5, w - 1, r.h - 1, RADIUS.lane - 0.5);
      ctx.lineWidth = STROKE.hairline;
      ctx.strokeStyle = TERM.edge;
      ctx.stroke();
      ctx.restore();
    }
    ctx.restore();
  }
  // The lines: one centred, or from +38 every 30; a hero line set large in
  // the grown lane's upper part, clear of its stamp. Each prints left to
  // right over a quarter beat (a flown one over its queued self), faded
  // over the lane's last 60 px.
  ctx.save();
  if (o.clip) {
    const c = o.clip;
    roundedRect(ctx, c.x, c.y, c.w, c.h, Math.min(RADIUS.lane, c.h / 2));
  } else roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.lane);
  ctx.clip();
  // A line never prints ahead of its strip: its reveal stops at the
  // growing strip's leading edge.
  const edge = strip && grow < 1 ? r.x + w : Infinity;
  l.lines.forEach((line, j) => {
    const k = span(b, line.at, DUR.tick, "arrive");
    const waiting = line.from ? b >= line.from.lands : false;
    if (k <= 0 && !waiting) return;
    const size = lerp(TYPE.lane.text, TYPE.lane.hero, hero);
    const x = r.x + TEXT_INSET;
    const y = geo.lineY(j);
    const wide = Math.min(
      (monoW(runsText(line.runs), size) + 8) * k,
      Math.max(0, edge - (x - 4)),
    );
    if (k > 0) {
      ctx.save();
      if (k < 1 || grow < 1) {
        ctx.beginPath();
        ctx.rect(x - 4, r.y, wide, r.h);
        ctx.clip();
      }
      mono(ctx, line.runs, x, y, size, { fade: geo.fade });
      ctx.restore();
    }
    if (waiting && (k < 1 || grow < 1)) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(x - 4 + wide, r.y, r.w, r.h);
      ctx.clip();
      ctx.globalAlpha *= QUEUED;
      mono(ctx, line.runs, x, y, size, { fade: geo.fade });
      ctx.restore();
    }
  });
  ctx.restore();
  ctx.restore();
}

/**
 * The depends bracket in the gutter between labels and tracks: a tick out
 * of each dependency's lane into a trunk at LANES_ART.bracketX, and a
 * rounded elbow into the root's lane ending in an arrowhead, drawn on to
 * `k`. It never crosses a lane's text: it runs left of the tracks and,
 * into the root, through the root's indent.
 */
function drawBracket(
  ctx: CanvasRenderingContext2D,
  boxes: readonly LaneBox[],
  root: number,
  k: number,
): void {
  if (k <= 0) return;
  const B = LANES_ART.bracket;
  const bx = LANES_ART.bracketX;
  const deps = boxes.filter((_, i) => i !== root);
  const ci = boxes[root];
  const top = Math.min(...deps.map((d) => d.mid));
  ctx.save();
  ctx.strokeStyle = rgba(B.color, B.alpha);
  ctx.fillStyle = rgba(B.color, B.alpha);
  ctx.lineWidth = STROKE.tree;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  // The ticks first, then the trunk down, then the elbow and its head.
  const ticks = clamp(k / 0.3);
  const trunk = clamp((k - 0.2) / 0.5);
  const elbow = clamp((k - 0.6) / 0.35);
  ctx.beginPath();
  for (const d of deps) {
    const x0 = d.rect.x - 4;
    const x1 = bx + ELBOW * 0.75;
    ctx.moveTo(x0, d.mid);
    ctx.lineTo(lerp(x0, x1, ticks), d.mid);
    if (ticks >= 1)
      ctx.arcTo(bx, d.mid, bx, d.mid + ELBOW * 0.75, ELBOW * 0.75);
  }
  if (trunk > 0) {
    const y0 = top + ELBOW * 0.75;
    ctx.moveTo(bx, y0);
    ctx.lineTo(bx, lerp(y0, ci.mid - ELBOW, trunk));
  }
  const tip = ci.rect.x - 4;
  if (elbow > 0) {
    ctx.moveTo(bx, ci.mid - ELBOW);
    ctx.arcTo(bx, ci.mid, bx + ELBOW, ci.mid, ELBOW);
    ctx.lineTo(lerp(bx + ELBOW, tip - B.arrow, elbow), ci.mid);
  }
  ctx.stroke();
  if (elbow >= 1) {
    ctx.beginPath();
    ctx.moveTo(tip, ci.mid);
    ctx.lineTo(tip - B.arrow, ci.mid - B.arrow * 0.6);
    ctx.lineTo(tip - B.arrow, ci.mid + B.arrow * 0.6);
    ctx.closePath();
    ctx.fill();
  }
  ctx.restore();
}
