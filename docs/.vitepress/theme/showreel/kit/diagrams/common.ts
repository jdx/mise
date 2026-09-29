// What the reel's diagrams share: timing on a section's beats, the raised
// surfaces and file cards they stand on, and the marks they carry (badges,
// annotation pills, value chips, ticks, rings, stamps, seat marks, threads),
// all per ART.md §3 and §7–9 with the numbers from kit/style.ts. Also the
// syntax colours a file's lines are set in (TOML and YAML, ART.md §2
// SYNTAX), and the "CAPTURE MISSING" card (§12.17).
//
// Mono is always set on the terminal's grid (drawTermLine, 0.6 em a
// column), never measured, so every string a diagram shows also reaches the
// ink tap (kit/ink.ts) the forbidden-text test reads. Nothing here keeps
// state: every function is a pure function of its arguments.
//
// The shadows, raised surfaces, badges, annotation pills, threads and beat
// spans are the shared kit's own (fx.ts castShadow, kit/parts.ts raised,
// annotation, threadAlong, kit/term.ts drawPill, kit/motion.ts span),
// re-exported here under the names the diagrams use, so a diagram's card
// casts the same shadow a scene's does and two neighbours look the same
// whichever is drawn first.

import { PALETTE, TERM } from "../../bible";
import { rgba } from "../../color";
import { castShadow, ring, roundedRect } from "../../fx";
import { clamp, progress } from "../../math";
import { drawText, font } from "../../type";
import { type Pt, type Rect, span } from "../motion";
import {
  raised as raisedPart,
  type RaisedOptions as RaisedPartOptions,
  threadAlong,
} from "../parts";
import {
  advance,
  drawPill,
  drawTermLine,
  pillWidth,
  run,
  type Run,
} from "../term";
import {
  BADGE,
  CHIP_ART,
  COLOR,
  DUR,
  FILE_ART,
  GLOW,
  MOTION,
  RADIUS,
  SHADOW,
  type ShadowSpec,
  STAMP,
  STROKE,
  SYNTAX,
  TYPE,
} from "../style";

export { castShadow } from "../../fx";
export { annotation, roundCorners } from "../parts";

// Timing, on a section's beats.

export { span } from "../motion";

/** A bump up from 0 and back to exactly 0 over beats `at` to `at + dur` (sin²). */
export function bumpOver(b: number, at: number, dur: number): number {
  const p = progress(at, at + dur, b);
  return p <= 0 || p >= 1 ? 0 : Math.sin(Math.PI * p) ** 2;
}

/**
 * How far into a cascade item `i` of `n` starts: DUR.stagger apart, capped
 * so the whole cascade starts within DUR.staggerMax (ART.md §13).
 */
export const staggerOf = (i: number, n: number): number =>
  n > 1 ? i * Math.min(DUR.stagger, DUR.staggerMax / (n - 1)) : 0;

/** A small mark popping in (scale 0.9 → 1 and a fade), from its entrance `k`. */
export const pop = (k: number): { alpha: number; scale: number } => ({
  alpha: clamp(k),
  scale: 0.9 + 0.1 * clamp(k),
});

/** Draw `fn` scaled by `s` about (cx, cy). */
export function scaled(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  s: number,
  fn: () => void,
): void {
  if (s === 1) {
    fn();
    return;
  }
  ctx.save();
  ctx.translate(cx, cy);
  ctx.scale(s, s);
  ctx.translate(-cx, -cy);
  fn();
  ctx.restore();
}

// Text.

/** A line's characters as the grid sets them. */
export const runsText = (runs: readonly Run[]): string =>
  runs.map((r) => r.text).join("");

/** How wide `text` is on the grid at `size` px: one advance a character. */
export const monoW = (text: string, size: number): number =>
  Array.from(text).length * advance(size);

/**
 * Mono on the terminal's grid: `line` as runs, or a string in one colour
 * (never styled as a prompt). Returns its width.
 */
export function mono(
  ctx: CanvasRenderingContext2D,
  line: string | readonly Run[],
  x: number,
  y: number,
  size: number,
  o: {
    color?: string;
    bold?: boolean;
    fade?: readonly [number, number] | null;
  } = {},
): number {
  const runs =
    typeof line === "string"
      ? [run(line, o.color ?? PALETTE.text1, o.bold ?? false)]
      : line;
  drawTermLine(ctx, runs, x, y, size, { fade: o.fade ?? null });
  return runs.reduce((w, r) => w + monoW(r.text, size), 0);
}

/** The baseline that centres `size` px mono in a row from `top` of height `h`. */
export const monoBaseline = (top: number, h: number, size: number): number =>
  top + (h - size) / 2 + 0.8 * size;

/** The baseline that centres Space Grotesk caps of `size` px on `midY`. */
export const typeBaseline = (midY: number, size: number): number =>
  midY + 0.36 * size;

/** Split runs at column `col`. */
export function splitAt(runs: readonly Run[], col: number): [Run[], Run[]] {
  const a: Run[] = [];
  const b: Run[] = [];
  let c = 0;
  for (const r of runs) {
    const chars = Array.from(r.text);
    const cut = clamp(col - c, 0, chars.length);
    if (cut > 0) a.push({ ...r, text: chars.slice(0, cut).join("") });
    if (cut < chars.length) b.push({ ...r, text: chars.slice(cut).join("") });
    c += chars.length;
  }
  return [a, b];
}

// File syntax (ART.md §2 SYNTAX): values are the brightest ink.

const PUNCT = /^[[\]{},]$/;
/** A key's `=` and the spaces round it (never a string that holds one). */
const EQ = /^\s*=\s*$/;

/** A TOML value's runs: strings and bare values in paper, brackets and commas in text3, inline keys in text2. */
function tomlValueRuns(v: string): Run[] {
  const out: Run[] = [];
  const tokens =
    v.match(
      /"(?:[^"\\]|\\.)*"|'[^']*'|\s*=\s*|[[\]{},]|\s+|[^\s"'[\]{},=]+/g,
    ) ?? [];
  tokens.forEach((tok, i) => {
    if (EQ.test(tok) || PUNCT.test(tok) || /^\s+$/.test(tok))
      out.push(run(tok, SYNTAX.punct));
    else {
      // A bare word followed by `=` is an inline table's key.
      const next = tokens.slice(i + 1).find((t) => !/^\s+$/.test(t));
      const key = !/^["']/.test(tok) && next !== undefined && EQ.test(next);
      out.push(run(tok, key ? SYNTAX.key : SYNTAX.value));
    }
  });
  return out;
}

/**
 * A TOML line's runs: a header in `header` (bold), a comment in text3, a
 * `key = value` as key text2, `=` text3, the value paper.
 */
export function tomlRuns(line: string, o: { header?: string } = {}): Run[] {
  const lead = /^\s*/.exec(line)?.[0] ?? "";
  const rest = line.slice(lead.length);
  if (rest.startsWith("[")) return [run(line, o.header ?? SYNTAX.header, true)];
  if (rest.startsWith("#")) return [run(line, SYNTAX.comment)];
  const m = /^("(?:[^"\\]|\\.)*"|[^\s=]+)(\s*=\s*)(.*)$/.exec(rest);
  if (!m) return [run(line, SYNTAX.value)];
  return [
    run(lead + m[1], SYNTAX.key),
    run(m[2], SYNTAX.punct),
    ...tomlValueRuns(m[3]),
  ];
}

/** A YAML line's runs: keys text2, values paper, `-`, `:` and flow brackets text3. */
export function yamlRuns(line: string): Run[] {
  const m = /^(\s*)(- )?(?:([^:#\s][^:#]*?)(:)(?=\s|$))?(.*)$/.exec(line);
  if (!m) return [run(line, SYNTAX.value)];
  const [, lead, dash, key, colon, value] = m;
  const out: Run[] = [];
  if (lead) out.push(run(lead, SYNTAX.punct));
  if (dash) out.push(run(dash, SYNTAX.punct));
  if (key) out.push(run(key, SYNTAX.key), run(colon, SYNTAX.punct));
  for (const tok of value.match(/[[\]{},]|[^[\]{},]+/g) ?? [])
    out.push(run(tok, PUNCT.test(tok) ? SYNTAX.punct : SYNTAX.value));
  return out.filter((r) => r.text);
}

// Shadows and raised surfaces (ART.md §3).

export interface RaisedOptions {
  /** The cast shadow (default SHADOW.raised); null for none. `contact` is added under it. */
  shadow?: ShadowSpec | null;
  /** A flat fill instead of the lit surface gradient. */
  fill?: string;
  edge?: string;
  edgeWidth?: number;
  /** The shadow's alpha multiplier (a card half lifted, a panel at 30 %). */
  shadowAlpha?: number;
}

/** A path for rounded rect `r`, for castShadow and clips. */
export const rectPath =
  (ctx: CanvasRenderingContext2D, r: Rect, radius: number) => (): void =>
    roundedRect(ctx, r.x, r.y, r.w, r.h, radius);

/**
 * A raised object's body (ART.md §3): kit/parts.ts raised, its cast and
 * contact shadows kept below its top edge and within SHADOW_SPILL of its
 * sides. `shape` outlines anything but a rounded rect (a folder with its
 * tab) inside `r`: its shadow, fill and edge.
 */
export function raised(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  radius: number,
  o: RaisedOptions = {},
  shape?: () => void,
): void {
  const po: RaisedPartOptions = { ...o, shape };
  raisedPart(ctx, r, radius, po);
}

export interface FileCardOptions extends RaisedOptions {
  tabH?: number;
  titleSize?: number;
  radius?: number;
  /** A micro badge in the tab strip's right end ("gitignored", the slip's excerpt note). */
  tag?: string;
  tagSize?: number;
}

/**
 * A file card (ART.md §8): a raised surface with a tab strip across its top
 * (TERM.window), the file's tab (the surface's own top colour, radius 10)
 * holding its name in mono text2, a hairline rule right of the tab, and an
 * optional micro badge in the strip's right end. Returns the body under
 * the strip.
 */
export function fileCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  title: string,
  o: FileCardOptions = {},
): Rect {
  const radius = o.radius ?? RADIUS.file;
  const tabH = o.tabH ?? FILE_ART.tabH;
  const size = o.titleSize ?? TYPE.file.title;
  raised(ctx, r, radius, o);
  const tabW = Math.min(r.w, monoW(title, size) + 2 * FILE_ART.tabPad);
  ctx.save();
  roundedRect(ctx, r.x, r.y, r.w, r.h, radius);
  ctx.clip();
  ctx.fillStyle = TERM.window;
  ctx.fillRect(r.x, r.y, r.w, tabH);
  roundedRect(ctx, r.x, r.y, tabW, tabH + RADIUS.tab, RADIUS.tab);
  ctx.fillStyle = COLOR.surfaceTop;
  ctx.fill();
  ctx.fillStyle = o.edge ?? PALETTE.divider;
  ctx.fillRect(r.x + tabW, r.y + tabH - 1, r.w - tabW, 1);
  ctx.fillStyle = COLOR.topEdge;
  ctx.fillRect(r.x, r.y + 1, r.w, 1);
  mono(ctx, title, r.x + FILE_ART.tabPad, monoBaseline(r.y, tabH, size), size, {
    color: PALETTE.text2,
  });
  ctx.restore();
  if (o.tag) {
    const ts = o.tagSize ?? TYPE.badgeMicro.size;
    const h = Math.round(ts * BADGE.heightEm);
    badge(ctx, r.x + r.w - 12, r.y + (tabH - h) / 2, o.tag, {
      size: ts,
      align: "right",
    });
  }
  return { x: r.x, y: r.y + tabH, w: r.w, h: r.h - tabH };
}

/**
 * The card a diagram draws where a take it needs is missing from the
 * capture set (ART.md §12.17): a file card whose one row is
 * "CAPTURE MISSING: <id>" in ANSI red.
 */
export function missingCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  id: string,
  alpha = 1,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const body = fileCard(ctx, r, "capture set");
  const size = TYPE.file.size;
  mono(
    ctx,
    `CAPTURE MISSING: ${id}`,
    body.x + FILE_ART.inset,
    monoBaseline(body.y + FILE_ART.top, TYPE.file.lineH, size),
    size,
    { color: TERM.ansi[1] },
  );
  ctx.restore();
}

// Marks (ART.md §9).

export interface BadgeOptions {
  size?: number;
  align?: "left" | "right" | "center";
  /** Its entrance, 0 to 1: a fade and a scale from 0.9 (DUR.badge, arrive). */
  k?: number;
  mono?: boolean;
}

/**
 * A badge (kit/term.ts drawPill): a pill 1.6 em tall, night at 85 % under a
 * paper outline (2 px, 1.5 px under 26 px), Space Grotesk 500 in paper
 * (mono for a captured string), popping in over `k`. Never a pillar
 * colour. Returns its width.
 */
export function badge(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  text: string,
  o: BadgeOptions = {},
): number {
  const size = o.size ?? TYPE.badge.size;
  const k = o.k ?? 1;
  if (k <= 0) return pillWidth(ctx, text, { size, mono: o.mono });
  const p = pop(k);
  ctx.save();
  ctx.globalAlpha *= p.alpha;
  const w = drawPill(ctx, x, y, text, {
    size,
    mono: o.mono,
    align: o.align ?? "left",
    scale: p.scale,
  });
  ctx.restore();
  return w;
}

/** A value chip's rect for `chars` characters of `size` px mono, its text on `baseline` from `x`. */
export function chipRect(
  chars: number,
  x: number,
  baseline: number,
  size: number,
): Rect {
  const h = Math.round(CHIP_ART.heightEm * size);
  const w = chars * advance(size) + size;
  // Centred on the caps (digits and the `v` sit at cap height).
  const mid = baseline - 0.36 * size;
  return { x, y: mid - h / 2, w, h };
}

/**
 * A value chip (ART.md §9): a lifted captured value in its capture's colours
 * on TERM.window, under a 2 px paper outline, radius 10, with the `small`
 * shadow. Returns its rect.
 */
export function valueChip(
  ctx: CanvasRenderingContext2D,
  runs: readonly Run[],
  x: number,
  baseline: number,
  size: number,
  o: { alpha?: number; shadow?: ShadowSpec | null } = {},
): Rect {
  const n = Array.from(runsText(runs)).length;
  const r = chipRect(n, x, baseline, size);
  const a = o.alpha ?? 1;
  if (a <= 0) return r;
  ctx.save();
  ctx.globalAlpha *= a;
  const path = rectPath(ctx, r, RADIUS.chip);
  const shadow = o.shadow === undefined ? SHADOW.small : o.shadow;
  if (shadow) castShadow(ctx, shadow, path);
  path();
  ctx.fillStyle = TERM.window;
  ctx.fill();
  ctx.lineWidth = STROKE.badge;
  ctx.strokeStyle = PALETTE.paper;
  roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, RADIUS.chip - 1);
  ctx.stroke();
  mono(ctx, runs, x + size / 2, baseline, size);
  ctx.restore();
  return r;
}

export interface PillOptions {
  /** Fill (default PALETTE.elevated). */
  fill?: string;
  /** Height, px (default 1.5 em). */
  h?: number;
  radius?: number;
  /** Text inset either side, px. */
  pad?: number;
  alpha?: number;
  edge?: string;
}

/** A mono chip on a flat pill (the slip's values, the drawn link's nodes). Returns its rect. */
export function pill(
  ctx: CanvasRenderingContext2D,
  runs: readonly Run[],
  x: number,
  baseline: number,
  size: number,
  o: PillOptions = {},
): Rect {
  const pad = o.pad ?? size / 2;
  const h = o.h ?? Math.round(CHIP_ART.heightEm * size);
  const w = monoW(runsText(runs), size) + 2 * pad;
  const r = { x, y: baseline - 0.36 * size - h / 2, w, h };
  const a = o.alpha ?? 1;
  if (a <= 0) return r;
  ctx.save();
  ctx.globalAlpha *= a;
  roundedRect(ctx, r.x, r.y, r.w, r.h, o.radius ?? RADIUS.seat);
  ctx.fillStyle = o.fill ?? PALETTE.elevated;
  ctx.fill();
  if (o.edge) {
    ctx.lineWidth = STROKE.badgeMicro;
    ctx.strokeStyle = o.edge;
    ctx.stroke();
  }
  mono(ctx, runs, x + pad, baseline, size);
  ctx.restore();
  return r;
}

/** A point `k` of the way along a polyline, and the polyline up to it. */
function along(pts: readonly Pt[], k: number): { upTo: Pt[]; head: Pt } {
  let total = 0;
  const seg: number[] = [];
  for (let i = 1; i < pts.length; i++) {
    const d = Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
    seg.push(d);
    total += d;
  }
  let left = clamp(k) * total;
  const upTo: Pt[] = [pts[0]];
  for (let i = 1; i < pts.length; i++) {
    const d = seg[i - 1];
    if (left >= d) {
      upTo.push(pts[i]);
      left -= d;
      continue;
    }
    const u = d > 0 ? left / d : 0;
    const p = {
      x: pts[i - 1].x + (pts[i].x - pts[i - 1].x) * u,
      y: pts[i - 1].y + (pts[i].y - pts[i - 1].y) * u,
    };
    upTo.push(p);
    return { upTo, head: p };
  }
  return { upTo, head: pts[pts.length - 1] };
}

/** Stroke a polyline up to `k` of its length; returns its head. */
export function strokeAlong(
  ctx: CanvasRenderingContext2D,
  pts: readonly Pt[],
  k: number,
): Pt | null {
  if (k <= 0 || pts.length < 2) return null;
  const { upTo, head } = along(pts, k);
  ctx.beginPath();
  ctx.moveTo(upTo[0].x, upTo[0].y);
  for (const p of upTo.slice(1)) ctx.lineTo(p.x, p.y);
  ctx.stroke();
  return head;
}

/**
 * A thread (ART.md §9, kit/parts.ts threadAlong): 2 px paper at 70 % with
 * round caps, routed through `pts` with THREAD.corner rounded corners,
 * drawn on to `k` of its length with a spark riding its head, and an
 * anchor dot at each end (the far one as it arrives).
 */
export function thread(
  ctx: CanvasRenderingContext2D,
  pts: readonly Pt[],
  k: number,
  alpha = 1,
): void {
  threadAlong(ctx, pts, k, alpha);
}

/**
 * A paper tick centred on (cx, cy), `s` px wide, drawn on to `k` (ART.md
 * §9: 5 px round, 4 px on chips).
 */
export function tick(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  s: number,
  k: number,
  width: number = STROKE.tick,
): void {
  if (k <= 0) return;
  const pts = [
    { x: cx - 0.46 * s, y: cy + 0.02 * s },
    { x: cx - 0.14 * s, y: cy + 0.32 * s },
    { x: cx + 0.48 * s, y: cy - 0.34 * s },
  ];
  ctx.save();
  ctx.strokeStyle = COLOR.mark;
  ctx.lineWidth = width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  strokeAlong(ctx, pts, k);
  ctx.restore();
}

/**
 * A seat mark (ART.md §7): an elevated band with a 1.5 px paper outline,
 * radius 8, over a row, at strength `k`.
 */
export function seatMark(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  k: number,
): void {
  if (k <= 0) return;
  ctx.save();
  ctx.globalAlpha *= clamp(k);
  roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.seat);
  ctx.fillStyle = PALETTE.elevated;
  ctx.fill();
  ctx.lineWidth = STROKE.seat;
  ctx.strokeStyle = COLOR.mark;
  ctx.stroke();
  ctx.restore();
}

/**
 * A stamp landing on beat `at` (ART.md §9): a paper outline rectangle
 * (4 px, radius 6, turned −2°) centred on (cx, cy), from 1.3× over
 * DUR.stamp (stamp), a glint-coloured fill at 25 % decaying over
 * DUR.flick, and one ring over half a beat. `text` sets its own word
 * inside it (Space Grotesk 34 / 700 paper); without it the stamp frames
 * what is under it.
 */
export function stamp(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  w: number,
  h: number,
  b: number,
  at: number,
  text?: string,
): void {
  if (b < at) return;
  const k = span(b, at, DUR.stamp, "stamp");
  const s = MOTION.stampFrom + (1 - MOTION.stampFrom) * k;
  const flash = STAMP.flash * (1 - progress(at, at + DUR.flick, b));
  ctx.save();
  ctx.globalAlpha *= clamp(k * 1.5);
  ctx.translate(cx, cy);
  ctx.rotate((STAMP.rotate * Math.PI) / 180);
  ctx.scale(s, s);
  roundedRect(ctx, -w / 2, -h / 2, w, h, STAMP.radius);
  if (flash > 0) {
    ctx.fillStyle = rgba(COLOR.glint, flash);
    ctx.fill();
  }
  ctx.lineWidth = STROKE.stamp;
  ctx.strokeStyle = COLOR.mark;
  ctx.stroke();
  if (text)
    drawText(ctx, text, 0, typeBaseline(0, TYPE.stamp.size), {
      font: font(TYPE.stamp.size, TYPE.stamp.weight),
      fill: COLOR.mark,
      align: "center",
    });
  ctx.restore();
  const rp = progress(at, at + DUR.half, b);
  ring(ctx, cx, cy, GLOW.ring.radius, rp, COLOR.mark, GLOW.ring.width);
}
