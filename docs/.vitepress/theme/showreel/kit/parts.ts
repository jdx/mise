// The kit's parts: where things stand, the raised surfaces every object on
// the stage is made of, badges, keycaps, footnotes, kinetic type, chips,
// threads, stamps and the other marks, the chef, the checkpoint rail, and
// the cards that show the takes' real files (ART.md §5–12, the numbers in
// kit/style.ts). Nothing here knows about scenes or bar lines, so both the
// scenes (kit/grey.ts) and the frames on the bar lines (handoff.ts, through
// kit/rest.ts) draw with the same calls, and a layer a scene carries across
// a bar line lands on the handoff's frame to the pixel.

import { PALETTE, PILLAR, type Pillar, TERM, W } from "../bible";
import { mix, rgba } from "../color";
import { castShadow, castShadows, glow, ring, roundedRect } from "../fx";
import { clamp, lerp, progress, smoothstep, TAU } from "../math";
import { drawText, font, layout, MONO } from "../type";
import { ink } from "./ink";
import { curveAt, type Pt, type Rect } from "./motion";
import {
  CARD_ART,
  CHIP_ART,
  COLOR,
  DASH,
  DUR,
  EASE,
  ENV_STRIP_ART,
  FILE_ART,
  GLOW,
  KEYCAP,
  LAYOUT,
  MOTION,
  PANES,
  PILLAR_BAND,
  PILOT,
  RADIUS,
  RAIL_ART,
  SHADOW,
  type ShadowSpec,
  SLOT,
  STAMP,
  STROKE,
  SYNTAX,
  THREAD,
  TYPE,
  UNDERLINE,
} from "./style";
import {
  advance,
  drawPill,
  drawTermLine,
  type Pane,
  pillHeight,
  type Run,
  run,
} from "./term";

/** 0 to 1 over beats `a` to `z` of the section, easing in and out over `f` beats at each end. */
export function win(b: number, a: number, z: number, f = 0.25): number {
  if (b < a || b > z) return 0;
  return smoothstep(a, a + f, b) * (1 - smoothstep(z - f, z, b));
}

/** 0 before beat `a`, then up to 1 over `f` beats. */
export const on = (b: number, a: number, f = 0.25): number =>
  smoothstep(a, a + f, b);

// Where things stand (kit/style.ts LAYOUT and PANES).

/** A terminal beside the station card: 56 columns of 26 px, 12 rows (PANES.side). */
export const LEFT: Pane = PANES.side;

/** The full-width terminal: 80 columns of 32 px, 11 rows (PANES.solo). */
export const FULL: Pane = PANES.solo;

/** The Environments act's terminal, under the shell-env strip (PANES.env). */
export const ENV_PANE: Pane = PANES.env;

/** The station card's place, right of a LEFT terminal. */
export const CARD: Rect = LAYOUT.card;

/** The station card in the Environments act, over the `.env` file card. */
export const ENV_CARD: Rect = LAYOUT.envCard;

/** The rect a pane's window covers. */
export const rectOf = (p: Pane): Rect => ({ x: p.x, y: p.y, w: p.w, h: p.h });

// Raised surfaces.

export interface RaisedOptions {
  /** A flat fill; otherwise the surface's gradient, lit from above. */
  fill?: string | CanvasGradient;
  /** The gradient's top and bottom (default COLOR.surfaceTop, surfaceBottom). */
  top?: string;
  bottom?: string;
  /** The edge's colour and width (default the divider, 1 px). */
  edge?: string;
  edgeWidth?: number;
  /** The cast shadow (default SHADOW.raised), or null for none; a contact shadow comes with it. */
  shadow?: ShadowSpec | null;
  /** The shadows' strength, 0 to 1 (a card half lifted, a panel at 30 %). */
  shadowAlpha?: number;
  alpha?: number;
  /**
   * An outline other than the rounded rect (a folder with its tab), built
   * as a path in the current transform and lying inside `r`: its shadow,
   * fill and edge. The edge then straddles the outline, clipped inside it.
   */
  shape?: () => void;
}

/**
 * A raised object's body (ART.md §3): its cast shadow and the tight contact
 * shadow under it, a fill lit from above, the 1 px highlight inside its top
 * edge, and a hairline edge. Every card, file card, tile, slip and panel is
 * one, the diagrams' too (kit/diagrams/common.ts).
 */
export function raised(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  radius: number,
  o: RaisedOptions = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0 || r.w <= 0 || r.h <= 0) return;
  const rad = Math.min(radius, r.w / 2, r.h / 2);
  const path = o.shape ?? (() => roundedRect(ctx, r.x, r.y, r.w, r.h, rad));
  ctx.save();
  ctx.globalAlpha *= a;
  if (o.shadow !== null)
    castShadows(
      ctx,
      [o.shadow ?? SHADOW.raised, SHADOW.contact],
      path,
      o.shadowAlpha ?? 1,
      r,
    );
  path();
  if (o.fill) ctx.fillStyle = o.fill;
  else {
    const g = ctx.createLinearGradient(0, r.y, 0, r.y + r.h);
    g.addColorStop(0, o.top ?? COLOR.surfaceTop);
    g.addColorStop(1, o.bottom ?? COLOR.surfaceBottom);
    ctx.fillStyle = g;
  }
  ctx.fill();
  ctx.save();
  ctx.clip();
  ctx.fillStyle = COLOR.topEdge;
  ctx.fillRect(r.x, r.y + 1, r.w, 1);
  ctx.restore();
  const ew = o.edgeWidth ?? STROKE.hairline;
  ctx.strokeStyle = o.edge ?? PALETTE.divider;
  if (o.shape) {
    // A custom outline's edge straddles it, clipped to its inside.
    o.shape();
    ctx.clip();
    ctx.lineWidth = 2 * ew;
    o.shape();
  } else {
    // Inside the rect, so a lit edge never grows the object.
    roundedRect(
      ctx,
      r.x + ew / 2,
      r.y + ew / 2,
      r.w - ew,
      r.h - ew,
      Math.max(0, rad - ew / 2),
    );
    ctx.lineWidth = ew;
  }
  ctx.stroke();
  ctx.restore();
}

// Mono on the grid, for files (never a terminal's line: those are drawTermLine's).

/**
 * Runs of a file's text on the 0.6 em grid, one character per column from
 * `x0`: a card's or file card's row. Only the first `chars` characters are
 * set (a row typing on); `fade` turns the text transparent across a card's
 * right edge instead of re-wrapping it.
 */
export function gridRuns(
  ctx: CanvasRenderingContext2D,
  runs: readonly Run[],
  x0: number,
  y: number,
  size: number,
  o: {
    chars?: number;
    fade?: readonly [number, number] | null;
    weight?: number;
  } = {},
): number {
  const adv = advance(size);
  const chars = o.chars ?? Infinity;
  const text = runs.map((r) => r.text).join("");
  const n = Array.from(text).length;
  if (chars <= 0) return n * adv;
  ink(Array.from(text).slice(0, chars).join(""), "text");
  ctx.save();
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  let col = 0;
  for (const r of runs) {
    let fill: string | CanvasGradient = r.color;
    if (o.fade) {
      const g = ctx.createLinearGradient(o.fade[0], 0, o.fade[1], 0);
      g.addColorStop(0, r.color);
      g.addColorStop(1, rgba(r.color, 0));
      fill = g;
    }
    ctx.fillStyle = fill;
    ctx.font = font(size, o.weight ?? (r.bold ? 700 : 400), MONO);
    for (const ch of r.text) {
      if (col >= chars) break;
      if (ch !== " ") ctx.fillText(ch, x0 + col * adv, y);
      col++;
    }
  }
  ctx.restore();
  return n * adv;
}

/** Plain runs in one colour. */
const plainRuns = (text: string, color: string, bold = false): Run[] => [
  run(text, color, bold),
];

/** Append to a list of runs, merging with the last when the colour matches. */
function pushRun(out: Run[], text: string, color: string, bold = false) {
  if (!text) return;
  const last = out.at(-1);
  if (last && last.color === color && last.bold === bold) last.text += text;
  else out.push(run(text, color, bold));
}

/**
 * A TOML line in the file syntax (kit/style.ts SYNTAX): keys text2,
 * `=` and brackets text3, values paper (the brightest ink on a card),
 * comments text3. Headers are set by the card itself.
 */
export function tomlRuns(line: string): Run[] {
  const out: Run[] = [];
  let rest = line;
  const m = /^(\s*)((?:"[^"]*"|[^\s="#[\]])+)(\s*=\s*)/.exec(line);
  if (m) {
    pushRun(out, m[1] + m[2], SYNTAX.key);
    pushRun(out, m[3], SYNTAX.punct);
    rest = line.slice(m[0].length);
  } else if (/^\s*\[/.test(line)) {
    pushRun(out, line, SYNTAX.headerOpen, true);
    return out;
  }
  for (const [tok] of rest.matchAll(
    /"(?:[^"\\]|\\.)*"?|'[^']*'?|#.*$|[[\]{},]|\s+|[^"'#[\]{},\s]+/g,
  )) {
    if (tok.startsWith("#")) pushRun(out, tok, SYNTAX.comment);
    else if (/^[[\]{},]$/.test(tok) || /^\s+$/.test(tok))
      pushRun(out, tok, SYNTAX.punct);
    else pushRun(out, tok, SYNTAX.value);
  }
  return out;
}

/** A `.env` line: the key text2, `=` text3, the value paper. */
export function envRuns(line: string): Run[] {
  const m = /^(\s*[A-Za-z_][A-Za-z0-9_]*)(=)(.*)$/.exec(line);
  if (!m)
    return plainRuns(line, /^\s*#/.test(line) ? SYNTAX.comment : PALETTE.text1);
  const out: Run[] = [];
  pushRun(out, m[1], SYNTAX.key);
  pushRun(out, m[2], SYNTAX.punct);
  pushRun(out, m[3], SYNTAX.value);
  return out;
}

/** A YAML line: keys text2, `-` and `:` text3, values paper, comments text3. */
export function yamlRuns(line: string): Run[] {
  const out: Run[] = [];
  const m = /^(\s*)(- )?(?:([^:#\s][^:#]*?)(:)(?=\s|$))?(.*)$/.exec(line)!;
  pushRun(out, m[1], SYNTAX.punct);
  if (m[2]) pushRun(out, m[2], SYNTAX.punct);
  if (m[3]) {
    pushRun(out, m[3], SYNTAX.key);
    pushRun(out, m[4], SYNTAX.punct);
  }
  const rest = m[5];
  const c = rest.search(/(^|\s)#/);
  if (c >= 0) {
    pushRun(out, rest.slice(0, c), SYNTAX.value);
    pushRun(out, rest.slice(c), SYNTAX.comment);
  } else pushRun(out, rest, SYNTAX.value);
  return out;
}

/**
 * A shell file's line: comments and the shebang text3, `#MISE` and
 * `#USAGE` directives (a task's own metadata) as a key in text2 and their
 * body in paper, anything else text1.
 */
export function shellRuns(line: string): Run[] {
  const d = /^(#(?:MISE|USAGE))(.*)$/.exec(line);
  if (d) {
    const out: Run[] = [];
    pushRun(out, d[1], SYNTAX.key);
    pushRun(out, d[2], SYNTAX.value);
    return out;
  }
  return plainRuns(line, /^\s*#/.test(line) ? SYNTAX.comment : PALETTE.text1);
}

/** How a file card colours a line, from its name. */
export function syntaxFor(title: string): (line: string) => Run[] {
  if (/\.(toml|lock)$/.test(title)) return tomlRuns;
  if (/(^|\/)\.env(\.[\w-]+)?$/.test(title)) return envRuns;
  if (/\.ya?ml$/.test(title)) return yamlRuns;
  return shellRuns;
}

// Annotations, meta labels, marks.

/**
 * An annotation pill (ART.md §2): Space Grotesk 22 / 500 text3 on an
 * elevated pill 30 px tall, centred on `midY`, from `x`. Never mono, so a
 * fold count cannot be read as the file's own text. Returns its width.
 */
export function annotation(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  midY: number,
  alpha = 1,
): number {
  const A = CARD_ART.annotation;
  const f = font(TYPE.annotation.size, TYPE.annotation.weight);
  const w = layout(ctx, text, f).width + 2 * A.pad;
  if (alpha <= 0) return w;
  ctx.save();
  ctx.globalAlpha *= alpha;
  roundedRect(ctx, x, midY - A.h / 2, w, A.h, A.h / 2);
  ctx.fillStyle = SYNTAX.annotationPill;
  ctx.fill();
  drawText(ctx, text, x + A.pad, midY + 0.36 * TYPE.annotation.size, {
    font: f,
    fill: SYNTAX.annotation,
  });
  ctx.restore();
  return w;
}

/** An uppercase meta label ("CHECKPOINTS", "ACT I", "SHELL ENV"): Space Grotesk 22 / 600, tracked out. */
export function meta(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  alpha = 1,
  fill: string = PALETTE.text3,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  drawText(ctx, text, x, y, {
    font: font(TYPE.meta.size, TYPE.meta.weight),
    fill,
    tracking: TYPE.meta.tracking * TYPE.meta.size,
  });
  ctx.restore();
}

/** A paper tick centred at (`cx`, `cy`), `size` px wide, drawn on over `k` (0 to 1). */
export function tick(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  size: number,
  k: number,
  o: { width?: number; color?: string } = {},
): void {
  if (k <= 0) return;
  const s = size / 34;
  const pts: Pt[] = [
    { x: cx - 16 * s, y: cy },
    { x: cx - 4 * s, y: cy + 12 * s },
    { x: cx + 18 * s, y: cy - 12 * s },
  ];
  const l1 = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
  const l2 = Math.hypot(pts[2].x - pts[1].x, pts[2].y - pts[1].y);
  const d = clamp(k) * (l1 + l2);
  ctx.save();
  ctx.strokeStyle = o.color ?? COLOR.mark;
  ctx.lineWidth = o.width ?? STROKE.tick;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  if (d <= l1)
    ctx.lineTo(
      lerp(pts[0].x, pts[1].x, d / l1),
      lerp(pts[0].y, pts[1].y, d / l1),
    );
  else {
    ctx.lineTo(pts[1].x, pts[1].y);
    const u = (d - l1) / l2;
    ctx.lineTo(lerp(pts[1].x, pts[2].x, u), lerp(pts[1].y, pts[2].y, u));
  }
  ctx.stroke();
  ctx.restore();
}

/** An underline (ART.md §9): 4 px, 16 px under the baseline `y`, drawn on left to right over `k`. */
export function underline(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  k: number,
  color: string,
  alpha = 1,
): void {
  if (k <= 0 || alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.fillStyle = color;
  ctx.fillRect(x, y + UNDERLINE.dy, w * clamp(k), STROKE.underline);
  ctx.restore();
}

/** A pointer (ART.md §9): a paper triangle 24 × 20, apex down on (`x`, `y`). */
export function pointer(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  alpha = 1,
): void {
  if (alpha <= 0) return;
  const P = RAIL_ART.pointer;
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.fillStyle = COLOR.mark;
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x - P.w / 2, y - P.h);
  ctx.lineTo(x + P.w / 2, y - P.h);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

/**
 * A seat mark (ART.md §7): an elevated band with a 1.5 px paper outline,
 * radius 8, round a row: a line that has just seated, or a file line lit.
 */
export function seatMark(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  alpha: number,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.seat);
  ctx.fillStyle = PALETTE.elevated;
  ctx.fill();
  roundedRect(ctx, r.x + 0.75, r.y + 0.75, r.w - 1.5, r.h - 1.5, RADIUS.seat);
  ctx.strokeStyle = COLOR.mark;
  ctx.lineWidth = STROKE.seat;
  ctx.stroke();
  ctx.restore();
}

/**
 * When a stamp lands, in beats after its `since` starts counting: its
 * approach from 1.3× takes DUR.stamp, and the flash and the ring start on
 * the landing. A scene's sound cue (the thunk) belongs at the stamp's start
 * plus STAMP_LAND, not at its start.
 */
export const STAMP_LAND = DUR.stamp;

/**
 * A stamp (ART.md §9) round `r`, `since` beats after its approach starts
 * (it lands at STAMP_LAND): a paper outline (4 px, radius 6) turned −2°,
 * landing from 1.3× over DUR.stamp (stamp), a glint-coloured flash inside
 * it decaying over DUR.flick, and one ring over half a beat from the
 * landing, growing from the stamp's own edge GLOW.ring.spread px out. Round
 * captured text it has no words of its own; with `label` ("skipped") it
 * sets them inside, Space Grotesk 34 / 700.
 */
export function stamp(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  since: number,
  o: { label?: string; alpha?: number } = {},
): void {
  const a = o.alpha ?? 1;
  if (since < 0 || a <= 0) return;
  const k = EASE.stamp(progress(0, DUR.stamp, since));
  const sc = lerp(MOTION.stampFrom, 1, k);
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  ctx.save();
  ctx.globalAlpha *= a * clamp(k * 3);
  ctx.translate(cx, cy);
  ctx.rotate((STAMP.rotate * Math.PI) / 180);
  ctx.scale(sc, sc);
  const flash =
    STAMP.flash * (1 - progress(DUR.stamp, DUR.stamp + DUR.flick, since));
  roundedRect(ctx, -r.w / 2, -r.h / 2, r.w, r.h, STAMP.radius);
  if (since >= DUR.stamp && flash > 0) {
    ctx.fillStyle = rgba(COLOR.glint, flash);
    ctx.fill();
  }
  ctx.strokeStyle = COLOR.mark;
  ctx.lineWidth = STROKE.stamp;
  ctx.stroke();
  if (o.label)
    drawText(ctx, o.label, 0, 0.36 * TYPE.stamp.size, {
      font: font(TYPE.stamp.size, TYPE.stamp.weight),
      fill: COLOR.mark,
      align: "center",
    });
  ctx.restore();
  const rp = progress(DUR.stamp, DUR.stamp + DUR.half, since);
  if (rp > 0 && rp < 1) {
    ctx.save();
    ctx.globalAlpha *= a;
    const edge = Math.max(r.w, r.h) / 2 + STROKE.stamp;
    ring(
      ctx,
      cx,
      cy,
      edge + GLOW.ring.spread,
      rp,
      COLOR.mark,
      GLOW.ring.width,
      edge,
    );
    ctx.restore();
  }
}

/**
 * Where a take a scene needs is missing from the capture set: a file card
 * whose one row is `CAPTURE MISSING: <id>` in ANSI red, standing where the
 * take's picture would (ART.md §12.17).
 */
export function missing(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  id: string,
  alpha = 1,
): void {
  if (alpha <= 0) return;
  const body = fileFrame(ctx, r, "capture set", { alpha });
  ctx.save();
  ctx.globalAlpha *= alpha;
  const size = Math.min(TYPE.file.size, Math.max(14, body.h * 0.4));
  drawTermLine(
    ctx,
    [run(`CAPTURE MISSING: ${id}`, TERM.ansi[1])],
    body.x + FILE_ART.inset,
    body.y + FILE_ART.top + 0.8 * size,
    size,
  );
  ctx.restore();
}

// Badges.

/**
 * A badge (ART.md §9): a pill of night at 85 % with a paper outline, never
 * a pillar colour. `mono` sets a captured string (a version a terminal
 * printed) in the terminal face; `scale` pops it in from 0.9. `y` is its
 * top. Returns its width.
 */
export function badge(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  text: string,
  alpha = 1,
  align: "left" | "right" | "center" = "left",
  o: { mono?: boolean; size?: number; scale?: number } = {},
): number {
  if (alpha <= 0) return 0;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const w = drawPill(ctx, x, y, text, {
    size: o.size,
    mono: o.mono,
    align,
    scale: o.scale,
  });
  ctx.restore();
  return w;
}

/** How a badge stands `since` beats after it came up: fading and scaling in from 0.9 over DUR.badge. */
export const badgePop = (since: number): { alpha: number; scale: number } => {
  const k = EASE.arrive(progress(0, DUR.badge, since));
  return { alpha: lerp(0.25, 1, k), scale: lerp(0.9, 1, k) };
};

// Keycaps.

/** Where a keycap's face stands on a pane (its top-left): straddling the pane's bottom-right (ART.md §6), `slot` keycaps to the left. */
export function keycapOn(
  p: Rect,
  key: "ctrl-l" | "tab",
  slot = 0,
): { x: number; y: number } {
  const k = key === "tab" ? KEYCAP.tab : KEYCAP.ctrlL;
  const cx = p.x + p.w + KEYCAP.dx - slot * KEYCAP.pair;
  const cy = p.y + p.h + KEYCAP.dy;
  return { x: cx - k.w / 2, y: cy - k.h / 2 };
}

/**
 * A keycap for a real keypress (ART.md §9), its face's top-left at (`x`,
 * `y`): ⌃L (its caret drawn as a vector) or Tab, a physical key with a lip
 * and a cast shadow. It rises in over the half beat before the key's real
 * time `at`, sinks on the press (DUR.keyDown), springs back (DUR.keyUp,
 * snap), holds, and drops out by `at + 1.5`.
 */
export function keycap(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  key: "ctrl-l" | "tab",
  b: number,
  at: number,
): void {
  const lead = DUR.keycapLead;
  const end = at + 1.5;
  if (b < at - lead || b >= end) return;
  const up = EASE.arrive(progress(at - lead, at, b));
  const out = EASE.leave(progress(end - DUR.exit, end, b));
  const a = up * (1 - out);
  if (a <= 0) return;
  const rise = 16 * (1 - up) + MOTION.fallOut * out;
  const down = EASE.settle(progress(at, at + DUR.keyDown, b));
  const back = EASE.snap(
    progress(at + DUR.keyDown, at + DUR.keyDown + DUR.keyUp, b),
  );
  const press = b < at ? 0 : down * (1 - back);
  const k = key === "tab" ? KEYCAP.tab : KEYCAP.ctrlL;
  const fy = y + rise + KEYCAP.press * press;
  const lipY = y + rise + KEYCAP.lip;
  const shadow: ShadowSpec = {
    ...SHADOW.small,
    alpha: lerp(SHADOW.small.alpha, SHADOW.pressed.alpha, clamp(press)),
    dy: lerp(SHADOW.small.dy, SHADOW.pressed.dy, clamp(press)),
    blur: lerp(SHADOW.small.blur, SHADOW.pressed.blur, clamp(press)),
  };
  ctx.save();
  ctx.globalAlpha *= a;
  // The lip under the face, and the key's shadow on the stage.
  castShadow(
    ctx,
    shadow,
    () => roundedRect(ctx, x, lipY, k.w, k.h, RADIUS.keycap),
    1,
    { x, y: fy, w: k.w, h: k.h + KEYCAP.lip },
  );
  roundedRect(ctx, x, lipY, k.w, k.h, RADIUS.keycap);
  ctx.fillStyle = COLOR.keyLip;
  ctx.fill();
  const g = ctx.createLinearGradient(0, fy, 0, fy + k.h);
  g.addColorStop(0, COLOR.keyTop);
  g.addColorStop(1, COLOR.keyBottom);
  roundedRect(ctx, x, fy, k.w, k.h, RADIUS.keycap);
  ctx.fillStyle = g;
  ctx.fill();
  roundedRect(ctx, x + 0.75, fy + 0.75, k.w - 1.5, k.h - 1.5, RADIUS.keycap);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = COLOR.keyEdge;
  ctx.stroke();
  ctx.fillStyle = rgba(PALETTE.paper, 0.12);
  ctx.fillRect(x + RADIUS.keycap, fy + 2, k.w - 2 * RADIUS.keycap, 1);
  const cx = x + k.w / 2;
  const base = fy + k.h / 2 + 0.36 * TYPE.keycap.size;
  if (key === "ctrl-l") {
    ctx.beginPath();
    ctx.moveTo(cx - 34, base - 16);
    ctx.lineTo(cx - 22, base - 30);
    ctx.lineTo(cx - 10, base - 16);
    ctx.lineWidth = STROKE.caret;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = PALETTE.paper;
    ctx.stroke();
    drawText(ctx, "L", cx + 16, base, {
      font: font(TYPE.keycap.size + 4, TYPE.keycap.weight),
      fill: PALETTE.paper,
      align: "center",
    });
  } else {
    drawText(ctx, "Tab", cx, base, {
      font: font(TYPE.keycap.size, TYPE.keycap.weight),
      fill: PALETTE.paper,
      align: "center",
    });
  }
  ctx.restore();
}

// Copy.

/** Copy with `code` in backticks, set left to right from `x`: prose in the type face, code in mono. */
export function codeLine(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  size: number,
  fill: string,
  codeFill: string,
): number {
  let at = x;
  text.split("`").forEach((part, i) => {
    if (!part) return;
    const f =
      i % 2 ? font(Math.round(size * 0.925), 500, MONO) : font(size, 500);
    at += drawText(ctx, part, at, y, {
      font: f,
      fill: i % 2 ? codeFill : fill,
    }).width;
  });
  return at - x;
}

/**
 * A footnote above the caption band (ART.md §9): Space Grotesk 40 / 500
 * paperDim at x 160 on baseline 728, code in mono 37 text2. It rises in a
 * little as it fades up, and sinks as it fades out. At most one per frame.
 */
export function footnote(
  ctx: CanvasRenderingContext2D,
  text: string,
  alpha: number,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const rise = Math.round((TYPE.footnote.size * 24) / 88) * (1 - alpha);
  codeLine(
    ctx,
    text,
    LAYOUT.left.x,
    LAYOUT.footnoteY + rise,
    TYPE.footnote.size,
    PALETTE.paperDim,
    PALETTE.text2,
  );
  ctx.restore();
}

// Kinetic type.

export interface BigOptions {
  /** Scale about the line's centre (motion.slam's). */
  scale?: number;
  /** The impact's glow behind it, 0 to 1 (motion.slam's). */
  impact?: number;
  /** Mono weight (default TYPE.big.weight, 700). */
  weight?: number;
}

/**
 * Kinetic type (ART.md §12.10): one line of mono 700, tracked in 2 %, big,
 * centred on `x` with its baseline on `y`. The scene drives its motion
 * (motion.slam: the approach from 1.4×, the impact's glow and squash, the
 * settle); `size` shrinks it into a pane's cells.
 */
export function bigline(
  ctx: CanvasRenderingContext2D,
  text: string,
  y: number,
  alpha: number,
  size: number = TYPE.big.climax,
  fill: string = PALETTE.paper,
  x = W / 2,
  o: BigOptions = {},
): void {
  if (alpha <= 0) return;
  const chars = Array.from(text);
  const adv = advance(size) + TYPE.big.tracking * size;
  const w = chars.length * adv - TYPE.big.tracking * size;
  const cy = y - 0.36 * size;
  ctx.save();
  ctx.globalAlpha *= alpha;
  if (o.impact && o.impact > 0)
    glow(
      ctx,
      x,
      cy,
      GLOW.slam.radius,
      PALETTE.paper,
      GLOW.slam.alpha * o.impact,
    );
  const sc = o.scale ?? 1;
  if (sc !== 1) {
    ctx.translate(x, cy);
    ctx.scale(sc, sc);
    ctx.translate(-x, -cy);
  }
  ink(text, "text");
  ctx.font = font(size, o.weight ?? TYPE.big.weight, MONO);
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = fill;
  chars.forEach((ch, i) => {
    if (ch !== " ") ctx.fillText(ch, x - w / 2 + i * adv, y);
  });
  ctx.restore();
}

/**
 * A captured line lifted into big type in the capture's own colours: its
 * runs, set on the terminal grid at `size`, centred on `cx`. Returns its
 * width.
 */
export function bigRuns(
  ctx: CanvasRenderingContext2D,
  runs: readonly Run[],
  cx: number,
  y: number,
  size: number,
  alpha: number,
): number {
  const n = runs.reduce((k, r) => k + Array.from(r.text).length, 0);
  const w = n * advance(size);
  if (alpha <= 0) return w;
  ctx.save();
  ctx.globalAlpha *= alpha;
  drawTermLine(ctx, runs, cx - w / 2, y, size);
  ctx.restore();
  return w;
}

/** Split a captured line's runs at column `col`: the text before it, and from it. */
export function splitRuns(runs: readonly Run[], col: number): [Run[], Run[]] {
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

// Threads.

/** Corners of a thread's route from (x0, y0) to (x1, y1): out of the source, along the gutter, into the target. */
function threadRoute(x0: number, y0: number, x1: number, y1: number): Pt[] {
  if (Math.abs(x1 - x0) < 24 || Math.abs(y1 - y0) < 4)
    return [
      { x: x0, y: y0 },
      { x: x1, y: y1 },
    ];
  const g = THREAD.gutterX;
  const across = (x0 < g && x1 > g) || (x1 < g && x0 > g);
  const tx = across ? g : (x0 + x1) / 2;
  return [
    { x: x0, y: y0 },
    { x: tx, y: y0 },
    { x: tx, y: y1 },
    { x: x1, y: y1 },
  ];
}

/** A polyline's corners rounded to `r`, sampled into points. */
export function roundCorners(pts: readonly Pt[], r: number): Pt[] {
  if (pts.length < 3 || r <= 0) return [...pts];
  const out: Pt[] = [pts[0]];
  for (let i = 1; i < pts.length - 1; i++) {
    const a = pts[i - 1];
    const p = pts[i];
    const c = pts[i + 1];
    const la = Math.hypot(p.x - a.x, p.y - a.y);
    const lc = Math.hypot(c.x - p.x, c.y - p.y);
    const rr = Math.min(r, la / 2, lc / 2);
    if (rr <= 0 || la === 0 || lc === 0) {
      out.push(p);
      continue;
    }
    const p0 = {
      x: p.x + ((a.x - p.x) / la) * rr,
      y: p.y + ((a.y - p.y) / la) * rr,
    };
    const p1 = {
      x: p.x + ((c.x - p.x) / lc) * rr,
      y: p.y + ((c.y - p.y) / lc) * rr,
    };
    for (let k = 0; k <= 8; k++) {
      const u = k / 8;
      out.push(curveAt({ a: p0, c: p, b: p1 }, u));
    }
  }
  out.push(pts[pts.length - 1]);
  return out;
}

/**
 * A thread (ART.md §9) from one point to another, drawn on over `k` (0 to
 * 1): 2 px paper at 70 %, round caps, an anchor dot at each end. It is
 * routed, not straight: out of the source horizontally, down or up the
 * gutter between the columns (or halfway, when both ends are on one side)
 * with 16 px rounded corners, and into the target; a thread between two
 * points in line runs straight. A spark rides its head while it draws.
 */
export function thread(
  ctx: CanvasRenderingContext2D,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  k: number,
  alpha = 1,
  color: string = THREAD.color,
): void {
  threadAlong(ctx, threadRoute(x0, y0, x1, y1), k, alpha, color);
}

/**
 * A thread (ART.md §9) along a route a scene or diagram chose: `corners`
 * rounded to THREAD.corner, drawn on to `k` of its length (2 px paper at
 * 70 %, round caps) with a spark riding its head, an anchor dot at its
 * start and one at its end as it arrives. `thread` routes one itself.
 */
export function threadAlong(
  ctx: CanvasRenderingContext2D,
  corners: readonly Pt[],
  k: number,
  alpha = 1,
  color: string = THREAD.color,
): void {
  if (alpha <= 0 || k <= 0 || corners.length < 2) return;
  const pts = roundCorners(corners, THREAD.corner);
  const last = pts[pts.length - 1];
  const lens = [0];
  for (let i = 1; i < pts.length; i++)
    lens.push(
      lens[i - 1] +
        Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y),
    );
  const total = lens[lens.length - 1];
  const d = clamp(k) * total;
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.strokeStyle = rgba(color, THREAD.alpha);
  ctx.lineWidth = STROKE.thread;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  let head = pts[0];
  for (let i = 1; i < pts.length; i++) {
    if (lens[i] <= d) {
      ctx.lineTo(pts[i].x, pts[i].y);
      head = pts[i];
      continue;
    }
    const seg = lens[i] - lens[i - 1];
    const u = seg > 0 ? (d - lens[i - 1]) / seg : 0;
    head = {
      x: lerp(pts[i - 1].x, pts[i].x, u),
      y: lerp(pts[i - 1].y, pts[i].y, u),
    };
    ctx.lineTo(head.x, head.y);
    break;
  }
  ctx.stroke();
  ctx.fillStyle = rgba(color, 0.9);
  ctx.beginPath();
  ctx.arc(pts[0].x, pts[0].y, STROKE.anchor, 0, TAU);
  ctx.fill();
  const end = smoothstep(0.9, 1, k);
  if (end > 0) {
    ctx.globalAlpha *= end;
    ctx.beginPath();
    ctx.arc(last.x, last.y, STROKE.anchor, 0, TAU);
    ctx.fill();
  }
  ctx.restore();
  if (k < 1) {
    ctx.save();
    ctx.globalAlpha *= alpha * (1 - smoothstep(0.85, 1, k));
    glow(ctx, head.x, head.y, GLOW.spark.size * 6, color, 0.7);
    ctx.fillStyle = mix(color, "#ffffff", 0.6);
    ctx.beginPath();
    ctx.arc(head.x, head.y, GLOW.spark.size * 0.8, 0, TAU);
    ctx.fill();
    ctx.restore();
  }
}

// Chips.

/**
 * A value chip (ART.md §9): a captured value lifted out of a terminal
 * (`v…`, `APP_ENV=api`), in mono at `size` in the capture's colour, on the
 * terminal's own fill with a 2 px paper outline, radius 10, casting a small
 * shadow. `y` is the text's baseline. Returns the chip's rect.
 */
export function chip(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  size: number,
  alpha: number,
  color: string = TERM.text,
): Rect {
  const w = Array.from(text).length * advance(size) + size;
  const h = Math.round(size * CHIP_ART.heightEm);
  const r = { x, y: y - h * 0.72, w, h };
  if (alpha <= 0) return r;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const path = () => roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.chip);
  castShadow(ctx, SHADOW.small, path, 1, r);
  path();
  ctx.fillStyle = TERM.window;
  ctx.fill();
  roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, RADIUS.chip - 1);
  ctx.lineWidth = STROKE.badge;
  ctx.strokeStyle = COLOR.mark;
  ctx.stroke();
  drawTermLine(ctx, [run(text, color)], x + size / 2, y, size);
  ctx.restore();
  return r;
}

/**
 * A backend chip (ART.md §9): `pypi:`, `cargo:`, `go:` in mono 36 text1 on
 * an elevated pill 56 px tall with a 1.5 px divider edge and a small
 * shadow; its top-left at (`x`, `y`). Returns its width.
 */
export function backendChip(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  alpha = 1,
): number {
  const size = TYPE.chip.backend;
  const h = CHIP_ART.backendH;
  const w = Array.from(text).length * advance(size) + 40;
  if (alpha <= 0) return w;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const path = () => roundedRect(ctx, x, y, w, h, h / 2);
  castShadow(ctx, SHADOW.small, path, 1, { x, y, w, h });
  path();
  ctx.fillStyle = PALETTE.elevated;
  ctx.fill();
  roundedRect(ctx, x + 0.75, y + 0.75, w - 1.5, h - 1.5, h / 2);
  ctx.strokeStyle = PALETTE.divider;
  ctx.lineWidth = 1.5;
  ctx.stroke();
  gridRuns(
    ctx,
    plainRuns(text, PALETTE.text1),
    x + 20,
    y + h / 2 + 0.36 * size,
    size,
    {
      weight: 500,
    },
  );
  ctx.restore();
  return w;
}

/**
 * A file chip (ART.md §9): README.md, `.github/workflows/ci.yml`,
 * `mise.lock`, in mono (26, or 32 for the lock) on a raised surface with a
 * document glyph before the name, and a paper tick after it drawn on over
 * `tick` (0 to 1) once a tool has checked it. Its top-left at (`x`, `y`).
 * Returns its rect.
 */
export function fileChip(
  ctx: CanvasRenderingContext2D,
  name: string,
  x: number,
  y: number,
  o: { size?: number; tick?: number; alpha?: number; shadow?: ShadowSpec } = {},
): Rect {
  const size = o.size ?? TYPE.chip.file;
  const G = CHIP_ART.docGlyph;
  const h = Math.round(size * 2.1);
  const tickW = (o.tick ?? 0) > 0 ? size + 8 : 0;
  const w =
    20 + G.w + 14 + Array.from(name).length * advance(size) + tickW + 22;
  const r = { x, y, w, h };
  const a = o.alpha ?? 1;
  if (a <= 0) return r;
  raised(ctx, r, RADIUS.chip, { shadow: o.shadow ?? SHADOW.small, alpha: a });
  ctx.save();
  ctx.globalAlpha *= a;
  // The document glyph: an outline with a folded corner.
  const gx = x + 20;
  const gy = y + h / 2 - G.h / 2;
  ctx.strokeStyle = PALETTE.text3;
  ctx.lineWidth = 2;
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(gx, gy);
  ctx.lineTo(gx + G.w - G.fold, gy);
  ctx.lineTo(gx + G.w, gy + G.fold);
  ctx.lineTo(gx + G.w, gy + G.h);
  ctx.lineTo(gx, gy + G.h);
  ctx.closePath();
  ctx.moveTo(gx + G.w - G.fold, gy);
  ctx.lineTo(gx + G.w - G.fold, gy + G.fold);
  ctx.lineTo(gx + G.w, gy + G.fold);
  ctx.stroke();
  const tx = gx + G.w + 14;
  const tw = gridRuns(
    ctx,
    plainRuns(name, PALETTE.text1),
    tx,
    y + h / 2 + 0.36 * size,
    size,
    {
      weight: 500,
    },
  );
  if ((o.tick ?? 0) > 0)
    tick(ctx, tx + tw + 4 + size / 2, y + h / 2, size * 0.9, o.tick ?? 0, {
      width: 4,
    });
  ctx.restore();
  return r;
}

// The chef.

// Files.

interface Block {
  header: string;
  body: string[];
}

/** A TOML file's tables: each header with the non-blank lines under it. */
export function tomlBlocks(text: string): Block[] {
  const out: Block[] = [];
  let cur: Block | null = null;
  for (const line of text.split("\n")) {
    if (/^\s*\[/.test(line)) {
      cur = { header: line.trim(), body: [] };
      out.push(cur);
    } else if (cur && line.trim()) cur.body.push(line);
  }
  return out;
}

/** The pillar a table belongs to, from its header. */
export function pillarOf(header: string): Pillar | null {
  if (/^\[\[?tools/.test(header)) return "tools";
  if (/^\[env/.test(header)) return "env";
  if (/^\[(tasks|daemons)/.test(header)) return "tasks";
  if (/^\[(dotfiles|bootstrap)/.test(header)) return "machine";
  return null;
}

/** A file card's frame: where its rows go. */
export interface FileFrame extends Rect {
  /** The tab strip's bottom. */
  tabBottom: number;
}

/**
 * A file card's box (ART.md §8): a raised surface, radius 14, a 46 px tab
 * strip of the terminal's fill with the file's name in mono 22 text2 on a
 * surface tab, a divider rule right of the tab, and a micro badge (`tag`,
 * "gitignored") in the strip's right end. Returns the body under the strip.
 */
export function fileFrame(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  title: string,
  o: {
    alpha?: number;
    tag?: string;
    tabH?: number;
    titleSize?: number;
    radius?: number;
    edge?: string;
    edgeWidth?: number;
    shadow?: ShadowSpec | null;
  } = {},
): FileFrame {
  const tabH = Math.min(o.tabH ?? FILE_ART.tabH, r.h);
  const body: FileFrame = {
    x: r.x,
    y: r.y + tabH,
    w: r.w,
    h: r.h - tabH,
    tabBottom: r.y + tabH,
  };
  const a = o.alpha ?? 1;
  if (a <= 0 || r.w <= 0 || r.h <= 0) return body;
  const rad = Math.min(o.radius ?? RADIUS.file, r.w / 2, r.h / 2);
  raised(ctx, r, rad, {
    alpha: a,
    edge: o.edge,
    edgeWidth: o.edgeWidth,
    shadow: o.shadow,
  });
  ctx.save();
  ctx.globalAlpha *= a;
  roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, Math.max(0, rad - 1));
  ctx.clip();
  ctx.fillStyle = TERM.window;
  ctx.fillRect(r.x, r.y, r.w, tabH);
  const ts = o.titleSize ?? TYPE.file.title;
  const tf = font(ts, 500, MONO);
  const tw = Math.min(r.w, layout(ctx, title, tf).width + 2 * FILE_ART.tabPad);
  roundedRect(ctx, r.x, r.y, tw, tabH + RADIUS.tab + 4, RADIUS.tab);
  ctx.fillStyle = COLOR.surfaceTop;
  ctx.fill();
  ctx.fillStyle = PALETTE.divider;
  ctx.fillRect(r.x + tw, r.y + tabH - 1, r.w - tw, 1);
  ctx.fillStyle = COLOR.topEdge;
  ctx.fillRect(r.x, r.y + 1, r.w, 1);
  drawText(ctx, title, r.x + FILE_ART.tabPad, r.y + tabH / 2 + 0.36 * ts, {
    font: tf,
    fill: PALETTE.text2,
  });
  ctx.restore();
  if (o.tag) {
    const ph = pillHeight(TYPE.badgeMicro.size);
    badge(ctx, r.x + r.w - 12, r.y + (tabH - ph) / 2, o.tag, a, "right", {
      size: TYPE.badgeMicro.size,
    });
  }
  // The edge again over the tab strip, which covered its top.
  ctx.save();
  ctx.globalAlpha *= a;
  const ew = o.edgeWidth ?? STROKE.hairline;
  roundedRect(
    ctx,
    r.x + ew / 2,
    r.y + ew / 2,
    r.w - ew,
    r.h - ew,
    Math.max(0, rad - ew / 2),
  );
  ctx.strokeStyle = o.edge ?? PALETTE.divider;
  ctx.lineWidth = ew;
  ctx.stroke();
  ctx.restore();
  return body;
}

export interface CardOptions {
  /** The file's path, on its tab. */
  title: string;
  /** The file as a take left it, or null to draw the missing card. */
  text: string | null;
  /** The take it comes from, named when it is missing. */
  from: string;
  /** Tables shown open, by header prefix; the rest fold to their header and a line count. */
  open?: readonly string[] | "all";
  /**
   * Tables part way through folding or unfolding, by header prefix: 0 is
   * folded, 1 open. Overrides `open` for the tables it names.
   */
  unfold?: Readonly<Record<string, number>>;
  /** Lines of an open table folded to a count instead (a line too long for the card). */
  hide?: RegExp;
  /** How lit each table's header is, by header prefix (in its pillar's colour). */
  lit?: Readonly<Record<string, number>>;
  /** The ignite pulse at a header's bar as it lights, 0 to 1, by header prefix. */
  ignite?: Readonly<Record<string, number>>;
  /**
   * Lines that have just seated, with how bright their seat mark is; with
   * `drop` (0 to 1) the line is still landing: the rows below part for it
   * over its first third, and it drops 28 px into its slot over the rest.
   */
  seat?: readonly { re: RegExp; a: number; drop?: number }[];
  /** 0 to 1: how much of the card has written on, top to bottom, each row typing on. */
  reveal?: number;
  alpha?: number;
  size?: number;
  /**
   * Size the card to its rows (cardFit): its bottom edge follows the rows'
   * own heights as tables fold, unfold and seat, down to `min` px (default
   * a tab and two rows), never past `r`. Off by default: a card on a bar
   * line's rest must fit the same way on both sides (kit/rest.ts CardRest
   * `fit`).
   */
  fit?: boolean | { min?: number };
}

/** Where a card set its rows: each row's text, baseline and left edge (and its kind, width and box). */
export type CardRows = {
  text: string;
  y: number;
  x: number;
  kind?: "header" | "body" | "fold";
  /** The text's width, px. */
  w?: number;
  /** The row's box, lineH tall. */
  top?: number;
  h?: number;
}[];

interface CardRow {
  kind: "header" | "body" | "fold";
  text: string;
  header: string;
  /** A folded header's annotation. */
  note?: string;
  /** How far open its table is: a body row's height and ink, a header's annotation fading out. */
  k: number;
  /**
   * A header splitting out of a fold of task tables (splitRows): its row's
   * height and ink, 0 to 1. Absent for a row that stands at rest.
   */
  grow?: number;
  /** The annotation it had before a table split out of its fold ("2 more tables"), swapping to `note`. */
  noteFrom?: string;
  /** How far that swap has gone, 0 to 1. */
  swap?: number;
}

const prefixed = (
  table: Readonly<Record<string, number>> | undefined,
  header: string,
): number | undefined => {
  let v: number | undefined;
  for (const [pre, x] of Object.entries(table ?? {}))
    if (header.startsWith(pre)) v = Math.max(v ?? 0, x);
  return v;
};

/**
 * A card's rows, with any task table splitting out of a fold of task
 * tables animated (ART.md §7): while a table named in `unfold` is part way
 * open and would be folded into the tables before it at 0, the rows are
 * the open layout's, but every header that the folded layout lacks (the
 * table splitting out, and a fold it leaves behind it) grows in with it,
 * and a fold whose count changes swaps its pill ("2 more tables" out,
 * "1 more table" in). So the card changes as one object: nothing
 * dissolves over a second layout.
 */
function cardRows(text: string, o: CardOptions, cols: number): CardRow[] {
  const at1 = cardRowsAt(text, o, cols);
  const task = /^\[tasks\./;
  const splitting = Object.entries(o.unfold ?? {}).filter(
    ([h, k]) => task.test(h) && k > 0 && k < 1,
  );
  if (!splitting.length) return at1;
  const g = Math.max(...splitting.map(([, k]) => k));
  const at0 = cardRowsAt(
    text,
    {
      ...o,
      unfold: {
        ...o.unfold,
        ...Object.fromEntries(splitting.map(([h]) => [h, 0])),
      },
    },
    cols,
  );
  const before = new Map(
    at0.filter((r) => r.kind === "header").map((r) => [r.header, r]),
  );
  return at1.map((r) => {
    if (r.kind !== "header") return r;
    const was = before.get(r.header);
    if (!was) return { ...r, grow: g };
    if (was.note !== r.note) return { ...r, noteFrom: was.note, swap: g };
    return r;
  });
}

/** A card's rows: open tables line by line, folded tables as a header and a count. */
function cardRowsAt(text: string, o: CardOptions, cols: number): CardRow[] {
  const rows: CardRow[] = [];
  const openK = (h: string): number => {
    const u = prefixed(o.unfold, h);
    if (u !== undefined) return clamp(u);
    return o.open === "all" || (o.open ?? []).some((p) => h.startsWith(p))
      ? 1
      : 0;
  };
  const blocks = tomlBlocks(text);
  const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
  for (let i = 0; i < blocks.length; i++) {
    const blk = blocks[i];
    const k = openK(blk.header);
    if (k > 0) {
      const hidden = blk.body.filter(
        (l) => Array.from(l).length > cols || (o.hide ? o.hide.test(l) : false),
      );
      rows.push({
        kind: "header",
        text: blk.header,
        header: blk.header,
        note: k < 1 ? plural(blk.body.length, "line") : undefined,
        k,
      });
      for (const l of blk.body)
        if (!hidden.includes(l))
          rows.push({ kind: "body", text: l, header: blk.header, k });
      if (hidden.length)
        rows.push({
          kind: "fold",
          text: plural(hidden.length, "more line"),
          header: blk.header,
          k,
        });
      continue;
    }
    // Folded task tables fold together, to one row: the first, and a count.
    const task = /^\[tasks\./.test(blk.header);
    let j = i;
    while (
      task &&
      j + 1 < blocks.length &&
      /^\[tasks\./.test(blocks[j + 1].header) &&
      openK(blocks[j + 1].header) <= 0
    )
      j++;
    const n = blk.body.length;
    rows.push({
      kind: "header",
      text: blk.header,
      header: blk.header,
      note:
        j > i
          ? `${j - i} more table${j - i === 1 ? "" : "s"}`
          : n
            ? plural(n, "line")
            : undefined,
      k: 0,
    });
    i = j;
  }
  return rows;
}

/**
 * The station card (ART.md §7): a mise.toml as a take left it, folded to
 * the tables the act needs. A raised surface with a tab strip holding the
 * file's path; rows in the file syntax on the 0.6 em grid; a folded table
 * is its header and an annotation pill ("4 lines", "3 more tables"); a line
 * wider than the card folds to "1 more line". A lit header gets its
 * pillar's band sweeping in, a bar down its left edge and its text in the
 * pillar's colour; a seated line its seat mark. `[daemons]` carries its
 * pilot light. Returns where it set its rows.
 */
export function card(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: CardOptions,
): CardRows {
  const alpha = o.alpha ?? 1;
  const placed: CardRows = [];
  if (alpha <= 0) return placed;
  if (o.text === null) {
    missing(ctx, r, o.from, alpha);
    return placed;
  }
  const size = o.size ?? TYPE.card.size;
  const lh = Math.round((size * TYPE.card.lineH) / TYPE.card.size);
  const adv = advance(size);
  const A = CARD_ART;
  const cols = Math.floor((r.w - 2 * A.inset) / adv + 1e-9);
  if (o.fit) r = cardFit(r, o);
  const body = fileFrame(ctx, r, o.title, {
    alpha,
    tabH: A.tabH,
    titleSize: TYPE.card.title,
    radius: RADIUS.card,
  });
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.beginPath();
  ctx.rect(r.x, body.y, r.w, Math.max(0, body.h - 2));
  ctx.clip();
  const rows = cardRows(o.text, o, cols);
  const shown = (o.reveal ?? 1) * rows.length;
  const x = r.x + A.inset;
  let y = body.y + A.top + 0.8 * size;
  const fade = [r.x + r.w - A.inset - 24, r.x + r.w - 12] as const;
  const mid = (yy: number) => yy - 0.8 * size + size / 2;
  rows.forEach((row, i) => {
    if (i >= shown) return;
    const chars =
      i + 1 > shown
        ? Math.floor((shown - i) * Array.from(row.text).length)
        : Infinity;
    const grow = row.grow ?? 1;
    if (row.kind === "header" && i > 0) y += A.headerGap * grow;
    const seat = rowSeat(row, o);
    const part = clamp(seat.drop / 0.33);
    const land = clamp((seat.drop - 0.33) / 0.67);
    const hk = (row.kind === "header" ? grow : row.k) * part;
    const top = y - 0.8 * size - (lh - size) / 2;
    if (row.kind === "header" && grow <= 0) return;
    if (row.kind === "header") {
      ctx.save();
      // A header splitting out of its fold grows in: ink with its height.
      ctx.globalAlpha *= grow < 1 ? grow * grow : 1;
      const p = pillarOf(row.header);
      const lit = clamp(prefixed(o.lit, row.header) ?? 0);
      if (p && lit > 0) {
        const bw = (r.w - 2 * A.bandInset) * EASE.arrive(lit);
        ctx.fillStyle = PILLAR_BAND[p];
        ctx.fillRect(r.x + A.bandInset, top, bw, lh);
        const bh = lh * EASE.arrive(lit);
        ctx.fillStyle = PILLAR[p];
        ctx.fillRect(
          r.x + A.bandInset,
          top + (lh - bh) / 2,
          STROKE.headerBar,
          bh,
        );
        const ig = prefixed(o.ignite, row.header) ?? 0;
        if (ig > 0)
          glow(
            ctx,
            r.x + A.bandInset + STROKE.headerBar / 2,
            top + lh / 2,
            GLOW.ignite.radius,
            PILLAR[p],
            GLOW.ignite.alpha * ig,
          );
      }
      const base =
        row.note === undefined || row.k > 0 ? SYNTAX.headerOpen : SYNTAX.header;
      const col = p && lit > 0 ? mix(base, PILLAR[p], lit) : base;
      const hw = gridRuns(ctx, plainRuns(row.text, col, true), x, y, size, {
        chars,
      });
      if (chars === Infinity) {
        // A fold whose count changes as a table splits out swaps its pill:
        // the old count out over the first half, the new one in over the second.
        const sw = row.swap ?? 1;
        const ax = x + hw + A.annotation.gap;
        if (row.noteFrom && sw < 0.5)
          annotation(ctx, row.noteFrom, ax, mid(y), (1 - row.k) * (1 - 2 * sw));
        if (row.note && sw >= 0.5)
          annotation(
            ctx,
            row.note,
            ax,
            mid(y),
            (1 - row.k) * (row.swap === undefined ? 1 : 2 * sw - 1),
          );
      }
      if (/^\[daemons\]/.test(row.header) && chars === Infinity) {
        const px = r.x + r.w - PILOT.inset;
        const py = mid(y);
        ctx.save();
        ctx.lineWidth = PILOT.ring;
        ctx.strokeStyle = PALETTE.text3;
        ctx.globalAlpha *= 1 - lit;
        ctx.beginPath();
        ctx.arc(px, py, PILOT.r - PILOT.ring / 2, 0, TAU);
        ctx.stroke();
        ctx.restore();
        if (lit > 0) {
          glow(
            ctx,
            px,
            py,
            GLOW.pilot.radius,
            PILLAR.tasks,
            GLOW.pilot.alpha * lit,
          );
          ctx.save();
          ctx.globalAlpha *= lit;
          ctx.fillStyle = PILLAR.tasks;
          ctx.beginPath();
          ctx.arc(px, py, PILOT.r, 0, TAU);
          ctx.fill();
          ctx.restore();
        }
      }
      ctx.restore();
      placed.push({ text: row.text, y, x, kind: "header", w: hw, top, h: lh });
    } else if (row.kind === "fold") {
      if (hk > 0) {
        ctx.save();
        ctx.globalAlpha *= row.k;
        if (row.k < 1) {
          ctx.beginPath();
          ctx.rect(r.x, top, r.w, lh * hk);
          ctx.clip();
        }
        annotation(ctx, row.text, x, mid(y));
        ctx.restore();
        placed.push({ text: row.text, y, x, kind: "fold", top, h: lh });
      }
    } else if (hk > 0) {
      ctx.save();
      ctx.globalAlpha *= row.k < 1 ? row.k * row.k : 1;
      // A row folding or unfolding is cut to its own opening band, like a
      // blind: its glyphs never spill onto the rows it closes over.
      if (row.k < 1) {
        ctx.beginPath();
        ctx.rect(r.x, top, r.w, lh * hk);
        ctx.clip();
      }
      seatMark(
        ctx,
        {
          x: r.x + A.seatInset,
          y: top + 2,
          w: r.w - 2 * A.seatInset,
          h: lh - 4,
        },
        seat.a * land,
      );
      const dy = -MOTION.seatDrop * (1 - EASE.snap(land));
      ctx.globalAlpha *= seat.drop < 1 ? clamp(land * 2) : 1;
      const w = gridRuns(ctx, tomlRuns(row.text), x, y + dy, size, {
        chars,
        fade,
      });
      ctx.restore();
      placed.push({ text: row.text, y, x, kind: "body", w, top, h: lh });
    }
    y += lh * hk;
  });
  ctx.restore();
  return placed;
}

/** A body row's seat (CardOptions.seat): its mark's brightness and how far it has dropped in. */
function rowSeat(row: CardRow, o: CardOptions): { a: number; drop: number } {
  if (row.kind !== "body") return { a: 0, drop: 1 };
  return (o.seat ?? []).reduce<{ a: number; drop: number }>(
    (m, s) =>
      s.re.test(row.text)
        ? { a: Math.max(m.a, s.a), drop: Math.min(m.drop, s.drop ?? 1) }
        : m,
    { a: 0, drop: 1 },
  );
}

/**
 * The card at `r` sized to its rows (CardOptions.fit): the same top, sides
 * and tab, its bottom 18 px under the last row's box, at least `min` px
 * (default the tab and two rows), never taller than `r`. It follows the
 * rows as card() sets them, so a table folding, unfolding, seating or
 * writing on moves the bottom edge with it. A pure function of the options.
 */
export function cardFit(r: Rect, o: CardOptions): Rect {
  if (o.text === null) return r;
  const size = o.size ?? TYPE.card.size;
  const lh = Math.round((size * TYPE.card.lineH) / TYPE.card.size);
  const A = CARD_ART;
  const cols = Math.floor((r.w - 2 * A.inset) / advance(size) + 1e-9);
  const rows = cardRows(o.text, o, cols);
  const shown = (o.reveal ?? 1) * rows.length;
  // The first row's box starts this far under the tab (A.top less half the leading).
  let bottom = r.y + A.tabH + A.top - (lh - size) / 2;
  rows.forEach((row, i) => {
    if (i >= shown) return;
    const grow = row.grow ?? 1;
    if (row.kind === "header" && i > 0) bottom += A.headerGap * grow;
    const part = clamp(rowSeat(row, o).drop / 0.33);
    bottom += lh * (row.kind === "header" ? grow : row.k) * part;
  });
  const fit = typeof o.fit === "object" ? o.fit : {};
  const min = fit.min ?? A.tabH + A.top + 2 * lh;
  const h = Math.min(r.h, Math.max(min, Math.ceil(bottom + 18 - r.y)));
  return { ...r, h };
}

export interface FileCardOptions {
  title: string;
  lines: readonly string[] | null;
  from: string;
  /** A micro badge in the tab strip (plan v3: "gitignored"). */
  tag?: string;
  /** Lines lit, with how brightly: a seat mark round each. */
  hl?: readonly { re: RegExp; a: number }[];
  alpha?: number;
  size?: number;
}

/**
 * A file card (ART.md §8): a small file's lines, as a fixture or a take
 * left it, in the file syntax its name implies (TOML, `.env`, YAML, a shell
 * file), mono on the grid, a line wider than the card fading at its edge.
 * Returns each line's baseline.
 */
export function fileCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: FileCardOptions,
): CardRows {
  const alpha = o.alpha ?? 1;
  const placed: CardRows = [];
  if (alpha <= 0) return placed;
  if (o.lines === null) {
    missing(ctx, r, o.from, alpha);
    return placed;
  }
  const size = o.size ?? TYPE.file.size;
  const lh = Math.round((size * TYPE.file.lineH) / TYPE.file.size);
  const body = fileFrame(ctx, r, o.title, { alpha, tag: o.tag });
  const syntax = syntaxFor(o.title);
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.beginPath();
  ctx.rect(r.x, body.y, r.w, Math.max(0, body.h - 2));
  ctx.clip();
  const x = r.x + FILE_ART.inset;
  const fade = [r.x + r.w - 72, r.x + r.w - 12] as const;
  o.lines.forEach((l, i) => {
    const y = body.y + FILE_ART.top + 0.8 * size + i * lh;
    const top = y - 0.8 * size - (lh - size) / 2;
    const hl = (o.hl ?? []).reduce(
      (m, h) => (h.re.test(l) ? Math.max(m, h.a) : m),
      0,
    );
    seatMark(ctx, { x: r.x + 12, y: top + 2, w: r.w - 24, h: lh - 4 }, hl);
    const w = gridRuns(ctx, syntax(l), x, y, size, { fade });
    placed.push({ text: l, y, x, kind: "body", w, top, h: lh });
  });
  ctx.restore();
  return placed;
}

// The checkpoint rail.

/**
 * The checkpoint rail (ART.md §12.9): a raised panel with its "CHECKPOINTS"
 * label; a track; a machine-terracotta node per checkpoint, snapping in as
 * its `on` rises, with its real id and trigger under it; a paper pointer
 * over the node at `at` (a dot index); and a dashed arc drawn on from the
 * node a rollback restored (`from`) to the one it made.
 */
export function rail(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  dots: readonly { id: string; label: string; on: number; from?: number }[],
  at: number,
  alpha: number,
): void {
  if (alpha <= 0) return;
  const A = RAIL_ART;
  raised(ctx, r, RADIUS.panel, {
    alpha,
    shadow: { ...SHADOW.raised, alpha: 0.3 },
  });
  ctx.save();
  ctx.globalAlpha *= alpha;
  meta(ctx, "CHECKPOINTS", r.x + A.labelInset, r.y + 44);
  const y = r.y + A.y;
  const x0 = r.x + A.inset;
  const x1 = r.x + r.w - A.inset;
  ctx.strokeStyle = PALETTE.divider;
  ctx.lineWidth = STROKE.track;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x0, y);
  ctx.lineTo(x1, y);
  ctx.stroke();
  const slots = Math.max(3, dots.length);
  const step = (x1 - x0 - 80) / (slots - 1);
  const xAt = (i: number) => x0 + 40 + i * step;
  dots.forEach((d, i) => {
    if (d.on <= 0 || d.from === undefined) return;
    // The rollback's arc, from the checkpoint it restored, over the track.
    const a = { x: xAt(d.from), y: y - A.node.r - 10 };
    const b = { x: xAt(i), y: y - A.node.r - 10 };
    const k = {
      a,
      b,
      c: { x: (a.x + b.x) / 2, y: y - A.arcRise - A.node.r - 40 },
    };
    const u = EASE.arrive(clamp(d.on));
    ctx.save();
    ctx.setLineDash([...DASH.arc]);
    ctx.strokeStyle = COLOR.arc;
    ctx.lineWidth = STROKE.arc;
    ctx.beginPath();
    for (let s = 0; s <= 40; s++) {
      const p = curveAt(k, (s / 40) * u);
      if (s) ctx.lineTo(p.x, p.y);
      else ctx.moveTo(p.x, p.y);
    }
    ctx.stroke();
    ctx.restore();
  });
  dots.forEach((d, i) => {
    if (d.on <= 0) return;
    const x = xAt(i);
    const sc = EASE.snap(clamp(d.on));
    ctx.save();
    ctx.fillStyle = COLOR.surfaceTop;
    ctx.beginPath();
    ctx.arc(x, y, (A.node.r + A.node.ring) * sc, 0, TAU);
    ctx.fill();
    ctx.fillStyle = PILLAR.machine;
    ctx.beginPath();
    ctx.arc(x, y, A.node.r * sc, 0, TAU);
    ctx.fill();
    ctx.globalAlpha *= clamp(d.on);
    drawText(ctx, d.id, x, y + A.idY, {
      font: font(TYPE.rail.id, 600, MONO),
      fill: PALETTE.text1,
      align: "center",
    });
    drawText(ctx, d.label, x, y + A.triggerY, {
      font: font(TYPE.rail.trigger, 500, MONO),
      fill: PALETTE.text2,
      align: "center",
    });
    ctx.restore();
  });
  if (at >= 0) pointer(ctx, xAt(at), y - A.node.r - A.pointer.gap + 4);
  ctx.restore();
}

// Stage furniture a scene can reuse.

/**
 * A strip of terminal for the stage (a lane, the shell-env strip): the
 * terminal's fill, radius 12, its edge, a small shadow, and an optional
 * pillar bar down its left edge.
 */
export function strip(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: { alpha?: number; bar?: string; flash?: number; dashed?: boolean } = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0 || r.w <= 0 || r.h <= 0) return;
  ctx.save();
  ctx.globalAlpha *= a;
  const rad = Math.min(RADIUS.lane, r.w / 2, r.h / 2);
  const path = () => roundedRect(ctx, r.x, r.y, r.w, r.h, rad);
  if (o.dashed) {
    roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, rad);
    ctx.setLineDash([...DASH.waiting]);
    ctx.strokeStyle = PALETTE.text3;
    ctx.lineWidth = 1.5;
    ctx.stroke();
    ctx.restore();
    return;
  }
  castShadows(ctx, [SHADOW.small, SHADOW.contact], path, 1, r);
  path();
  ctx.fillStyle = mix(TERM.window, PALETTE.paper, o.flash ?? 0);
  ctx.fill();
  if (o.bar) {
    ctx.save();
    path();
    ctx.clip();
    ctx.fillStyle = o.bar;
    ctx.fillRect(r.x, r.y, STROKE.headerBar, r.h);
    ctx.restore();
  }
  roundedRect(ctx, r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1, rad - 0.5);
  ctx.strokeStyle = TERM.edge;
  ctx.lineWidth = STROKE.hairline;
  ctx.stroke();
  ctx.restore();
}

/**
 * The shell-env strip (ART.md §12.12): a strip of terminal over the
 * Environments act's pane with the env pillar's bar down its left edge and
 * its "SHELL ENV" label, holding the values the take printed as value chips
 * from x + 200, 16 px apart. Each chip has its own alpha and lift (a chip
 * folding off flips up 12 px as it fades). Returns the chips' rects.
 */
export function envStrip(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  chips: readonly { text: string; alpha?: number; dy?: number }[],
  alpha = 1,
): Rect[] {
  const E = ENV_STRIP_ART;
  const out: Rect[] = [];
  if (alpha <= 0) return out;
  strip(ctx, r, { alpha, bar: PILLAR.env });
  ctx.save();
  ctx.globalAlpha *= alpha;
  meta(ctx, "SHELL ENV", r.x + E.labelX, r.y + r.h / 2 + 8);
  let x = r.x + E.chipX;
  const baseline = r.y + r.h / 2 + 0.36 * E.chipSize;
  for (const c of chips) {
    const cr = chip(
      ctx,
      c.text,
      x,
      baseline + (c.dy ?? 0),
      E.chipSize,
      c.alpha ?? 1,
    );
    out.push(cr);
    x += cr.w + E.chipGap;
  }
  ctx.restore();
  return out;
}

/**
 * A slot opening for kinetic type (ART.md §12.10, `npm:`): a dashed paper
 * outline, radius 12, round `r`, going solid as `solid` rises (0 to 1).
 */
export function slot(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  alpha: number,
  solid = 0,
): void {
  if (alpha <= 0 || r.w <= 0 || r.h <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  roundedRect(ctx, r.x, r.y, r.w, r.h, SLOT.radius);
  ctx.strokeStyle = COLOR.mark;
  ctx.lineWidth = STROKE.lit;
  if (solid < 1) {
    ctx.save();
    ctx.globalAlpha *= 1 - solid;
    ctx.setLineDash([...DASH.waiting]);
    ctx.stroke();
    ctx.restore();
  }
  if (solid > 0) {
    ctx.globalAlpha *= solid;
    ctx.stroke();
  }
  ctx.restore();
}
