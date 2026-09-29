// Shared by the new machine's scenes (bootstrap, breath, clone): a take's
// screen in a pane that moves between two presets without its rows
// jumping, a file card flying off the terminal row that names it, and a
// captured value lifting off its row onto the card.
//
// A pane changing preset (PANES.side to PANES.aside) changes its text size,
// its pitch and how many rows it holds; kit/term.ts termLayout scrolls a
// bottom-anchored screen by whole rows, so a tweened pane would jump a row
// as its row count crosses an integer. Here the scroll is tweened too, so
// the bottom row stays on the pane's bottom row throughout. Each end
// scrolls as a terminal does (kit/rest.ts takeScroll): never back up when a
// confirm prompt's widget collapses. The text is the take's screen as
// captured; only its size and place change.

import { type Capture, screenAt, type ScreenOptions } from "../../captures";
import { valueChip } from "../../kit/diagrams/common";
import { cwdHop, type G, type Play } from "../../kit/grey";
import {
  bump,
  type Curve,
  curveAt,
  exit,
  lerpRect,
  type Pt,
  type Rect,
  span,
} from "../../kit/motion";
import { fileFrame, gridRuns, syntaxFor } from "../../kit/parts";
import {
  orderBadges,
  screenBadges,
  shownRows,
  takeScroll,
} from "../../kit/rest";
import {
  CHIP_ART,
  DUR,
  FILE_ART,
  MOTION,
  SHADOW,
  type ShadowSpec,
  TYPE,
} from "../../kit/style";
import {
  advance,
  type ChromeBadge,
  drawTerm,
  lerpPane,
  lineText,
  type Pane,
  paneRows,
  type Run,
  type TermLayout,
} from "../../kit/term";
import { clamp, lerp } from "../../math";

/** Pane `a` tweened toward `b` by `k` (0 to 1, eased by the caller). */
export const paneBetween = (a: Pane, b: Pane, k: number): Pane =>
  k <= 0 ? a : k >= 1 ? b : lerpPane(a, b, k);

/**
 * The first line take `c`'s screen at take time `ct` shows in pane `p`, as
 * a terminal scrolls (kit/rest.ts takeScroll): its bottom rows, but never
 * back up when the screen gets shorter (a confirm's Yes/No and hint rows
 * folding to one `? Yes` line leave blank rows at the bottom, which the
 * next output fills). The same first line kit/rest.ts drawScreen shows, so
 * a scene's last frame and its bar line's rest agree.
 */
export const scrollOfTake = (
  c: Capture,
  ct: number,
  p: Pane,
  screen: ScreenOptions = {},
): number =>
  p.anchor === "top"
    ? 0
    : takeScroll(c, ct, Math.floor(paneRows(p) + 1e-9), screen);

/**
 * The scroll of take `c`'s screen at take time `ct` in a pane tweened from
 * `a` to `b` by `k`: each end's own (scrollOfTake), in between the tween
 * of the two, so no row jumps as the row count changes.
 */
export function scrollBetween(
  c: Capture,
  ct: number,
  a: Pane,
  b: Pane,
  k: number,
  screen: ScreenOptions = {},
): number {
  return lerp(
    scrollOfTake(c, ct, a, screen),
    scrollOfTake(c, ct, b, screen),
    clamp(k),
  );
}

export interface ScreenShot {
  lines: ReturnType<typeof screenAt>["lines"];
  layout: TermLayout;
}

/**
 * Take `c` at take time `ct` in pane `p` (its text; the scene draws the
 * window), scrolled to `scroll` (default scrollOfTake: never back up): the
 * chrome bar's title, the cwd dot's hop (on the section's beats, given the
 * `plays` that map them to take time: kit/grey.ts cwdHop), and its badges
 * (the scene's, and the illustration badge its rows call for, sticky from
 * the first `you/` typed: kit/rest.ts screenBadges), in their ART.md §6
 * order.
 */
export function drawTake(
  g: G,
  c: Capture,
  ct: number,
  p: Pane,
  o: {
    scroll?: number;
    screen?: ScreenOptions;
    lineAlpha?: (i: number, lines: readonly string[]) => number;
    badges?: readonly (string | ChromeBadge)[];
    alpha?: number;
    /** 0 to 1: the pane's text and title dimmed (PANE_DIM), as out of focus. */
    dim?: number;
    plays?: readonly Play[];
  } = {},
): ScreenShot {
  const screen = o.screen ?? {};
  const sc = screenAt(c, ct, screen);
  const texts = sc.lines.map(lineText);
  const la = o.lineAlpha;
  const scroll = o.scroll ?? scrollOfTake(c, ct, p, screen);
  // While the scroll is between rows, the row going under the chrome fades
  // with the part of it still showing, so no sliver of glyph tops is left.
  const edge = (i: number) => (i < scroll ? clamp(1 - (scroll - i)) : 1);
  const layout = drawTerm(g.ctx, { ...p, window: false }, sc.lines, {
    t: g.t,
    alpha: o.alpha ?? 1,
    dim: o.dim,
    scroll,
    cursor: sc.cursor ?? undefined,
    lineAlpha: (i) => (la ? la(i, texts) : 1) * edge(i),
    hop:
      cwdHop(c, ct, o.plays ? { plays: o.plays, b: g.b } : undefined) ??
      undefined,
    badges: orderBadges([
      ...(o.badges ?? []),
      ...screenBadges(c, ct, shownRows(texts, p, scroll)),
    ]),
  });
  return { lines: sc.lines, layout };
}

/** How far through its flight a file card starts writing its lines in. */
const WRITE_FROM = 0.35;

/** A shadow part way from one spec to another. */
const shadowBetween = (
  a: ShadowSpec,
  b: ShadowSpec,
  k: number,
): ShadowSpec => ({
  color: a.color,
  alpha: lerp(a.alpha, b.alpha, k),
  dx: lerp(a.dx, b.dx, k),
  dy: lerp(a.dy, b.dy, k),
  blur: lerp(a.blur, b.blur, k),
});

/**
 * A file card flying off the terminal row that names it (ART.md §8
 * "Enter"): at `k` 0 it is the row's own rect, a tab with the file's name;
 * it travels `path` (from the row's centre to its rect's) to its rect,
 * growing, tilting up to MOTION.flightTilt degrees and settling level,
 * casting the `float` shadow that eases back to `raised` as it lands. Its
 * lines write in from 35 % of the way, once the card is tall enough to
 * start holding them, so it is never a long empty strip in the air. `k`
 * is the flight's eased progress; at 1 it is kit/parts.ts fileCard at `to`.
 */
export function flyingFile(
  ctx: CanvasRenderingContext2D,
  from: Rect,
  to: Rect,
  k: number,
  o: {
    title: string;
    lines: readonly string[];
    size: number;
    alpha?: number;
    path: Curve;
  },
): void {
  const alpha = o.alpha ?? 1;
  if (alpha <= 0 || k <= 0) return;
  const c = curveAt(o.path, k);
  const wh = lerpRect(from, to, k);
  const r = { x: c.x - wh.w / 2, y: c.y - wh.h / 2, w: wh.w, h: wh.h };
  const tilt = ((MOTION.flightTilt * Math.PI) / 180) * Math.sin(Math.PI * k);
  const land = clamp((k - 0.7) / 0.3);
  ctx.save();
  // The copy lifts off the row, which keeps its own: it comes up to full
  // over the first fifth of the flight.
  ctx.globalAlpha *= alpha * clamp(k / 0.2);
  ctx.translate(c.x, c.y);
  ctx.rotate(tilt);
  ctx.translate(-c.x, -c.y);
  const body = fileFrame(ctx, r, o.title, {
    shadow: shadowBetween(SHADOW.float, SHADOW.raised, land),
  });
  const write = clamp((k - WRITE_FROM) / (1 - WRITE_FROM));
  if (write > 0) {
    const lh = Math.round((o.size * TYPE.file.lineH) / TYPE.file.size);
    const syntax = syntaxFor(o.title);
    ctx.save();
    ctx.beginPath();
    ctx.rect(r.x, body.y, r.w, Math.max(0, body.h - 2));
    ctx.clip();
    ctx.globalAlpha *= write * write;
    const fade = [r.x + r.w - 72, r.x + r.w - 12] as const;
    o.lines.forEach((l, i) => {
      const y = body.y + FILE_ART.top + 0.8 * o.size + i * lh;
      gridRuns(ctx, syntax(l), r.x + FILE_ART.inset, y, o.size, { fade });
    });
    ctx.restore();
  }
  ctx.restore();
}

/**
 * A routed path through `points` with its corners rounded to `radius` px
 * (each corner a quadratic from `radius` before it to `radius` after),
 * walked at constant speed: `at(u)` is the point `u` (0 to 1) of the way
 * along it by length. A pure function of its points.
 */
function routed(points: readonly Pt[], radius: number): (u: number) => Pt {
  // The path as straight runs and rounded corners, densely sampled.
  const pts: Pt[] = [points[0]];
  const along = (a: Pt, z: Pt, d: number): Pt => {
    const len = Math.hypot(z.x - a.x, z.y - a.y) || 1;
    return {
      x: a.x + ((z.x - a.x) * d) / len,
      y: a.y + ((z.y - a.y) * d) / len,
    };
  };
  for (let i = 1; i < points.length - 1; i++) {
    const a = points[i - 1];
    const k = points[i];
    const z = points[i + 1];
    const r = Math.min(
      radius,
      Math.hypot(k.x - a.x, k.y - a.y) / 2,
      Math.hypot(z.x - k.x, z.y - k.y) / 2,
    );
    const p0 = along(k, a, r);
    const p2 = along(k, z, r);
    pts.push(p0);
    for (let j = 1; j <= 12; j++) {
      const u = j / 12;
      const v = 1 - u;
      pts.push({
        x: v * v * p0.x + 2 * v * u * k.x + u * u * p2.x,
        y: v * v * p0.y + 2 * v * u * k.y + u * u * p2.y,
      });
    }
  }
  pts.push(points[points.length - 1]);
  const acc = [0];
  for (let i = 1; i < pts.length; i++)
    acc.push(
      acc[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y),
    );
  const total = acc[acc.length - 1] || 1;
  return (u: number): Pt => {
    const d = clamp(u) * total;
    let i = 1;
    while (i < acc.length - 1 && acc[i] < d) i++;
    const seg = acc[i] - acc[i - 1] || 1;
    const f = (d - acc[i - 1]) / seg;
    return {
      x: lerp(pts[i - 1].x, pts[i].x, f),
      y: lerp(pts[i - 1].y, pts[i].y, f),
    };
  };
}

/** A captured value lifted off a terminal row onto the card (ART.md §9 "Lifting a value"). */
export interface ValueLift {
  /** The value's runs, as the take printed them (its colours). */
  runs: readonly Run[];
  /** Where it stands in the pane: its first column's left edge, its baseline, the pane's size. */
  from: Pt & { size: number };
  /** Where its chip's text sits once landed, and the chip's size. */
  to: Pt & { size: number };
  /** The beat its chip forms round it, on a frame its row shows. */
  at: number;
  /** The beat it leaves (falls and fades, DUR.exit). */
  out: number;
  /**
   * The flight's route between its two ends (text positions): the chip
   * travels it at constant speed (routed), so it can go round the card's
   * rows instead of across them.
   */
  via: readonly Pt[];
}

/** A lift route's corners, px. */
const ROUTE_RADIUS = 48;

/**
 * A value lifting off its terminal row onto the card (ART.md §9): the chip
 * forms round a copy of the captured run in place (its fill covering the
 * pane's own glyphs from its first frame: form, then rise), popping 0.9 → 1
 * over 1/16 beat; it rises 6 px over 1/8; it flies its route over 3/4 beat
 * (travel), growing from the pane's size to its own and casting the
 * `float` shadow as it flies, `small` once landed; it lands with a snap (a
 * 6 % pop over 1/4 beat). From `out` it falls and fades. Returns the chip's
 * rect, or null when not up.
 */
export function liftValue(
  ctx: CanvasRenderingContext2D,
  l: ValueLift,
  b: number,
): Rect | null {
  if (b < l.at || b >= l.out + DUR.exit) return null;
  const route = routed(
    [{ x: l.from.x, y: l.from.y - CHIP_ART.liftRise }, ...l.via, l.to],
    ROUTE_RADIUS,
  );
  const form = span(b, l.at, DUR.stagger, "arrive");
  const rise = span(b, l.at, DUR.flick, "settle");
  const flyAt = l.at + DUR.flick;
  const fly = span(b, flyAt, DUR.lift, "travel");
  const landed = span(b, flyAt + DUR.lift - DUR.flick, DUR.flick, "arrive");
  const gone = exit(b, l.out);
  const p =
    fly > 0
      ? route(fly)
      : { x: l.from.x, y: l.from.y - CHIP_ART.liftRise * rise };
  const size = lerp(l.from.size, l.to.size, fly);
  const pop =
    (0.9 + 0.1 * form) * (1 + 0.06 * bump(b, flyAt + DUR.lift, DUR.tick));
  const n = Array.from(l.runs.map((r) => r.text).join("")).length;
  const w = n * advance(size) + size;
  const cx = p.x - size / 2 + w / 2;
  const cy = p.y - 0.36 * size;
  ctx.save();
  ctx.translate(0, gone.dy);
  ctx.translate(cx, cy);
  ctx.scale(pop, pop);
  ctx.translate(-cx, -cy);
  const r = valueChip(ctx, l.runs, p.x - size / 2, p.y, size, {
    alpha: form * gone.alpha,
    shadow: shadowBetween(SHADOW.float, SHADOW.small, landed),
  });
  ctx.restore();
  return r;
}
