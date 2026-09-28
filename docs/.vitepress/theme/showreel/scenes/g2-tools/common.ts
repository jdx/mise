// What the Dev tools scenes (pitch, use, registry, backends) share: when a
// caption's word lands, a captured value lifting out of a terminal row
// onto the card row that asked for it (ART.md §9 "Lifting a value"), and
// kinetic type under the short pane shrinking into its cells (ART.md
// §12.10). Pure functions of the beat and what the scene hands them.

import { BEAT, type SectionId, TERM } from "../../bible";
import {
  type Capture,
  type CaptureId,
  CUT_ON,
  frameAfter,
  lastBefore,
  type ReelData,
  stepOf,
} from "../../captures";
import { mix } from "../../color";
import { castShadow, roundedRect } from "../../fx";
import { lerp, smoothstep } from "../../math";
import { captionsFor, type SceneEvents } from "../../storyboard";
import { WORD } from "../../type";
import { LAPSE, type Play, play, playLen, typed } from "../../kit/grey";
import { bump, type Pt, span } from "../../kit/motion";
import { threadAlong } from "../../kit/parts";
import {
  CHIP_ART,
  COLOR,
  DUR,
  LAYOUT,
  PANES,
  RADIUS,
  SHADOW,
  STROKE,
  THREAD,
  TYPE,
} from "../../kit/style";
import { advance, drawTermLine, type Run, run } from "../../kit/term";

export { perFacts, takeIn } from "../../kit/pace";

/** A file a take left, or null: as captures.ts fileOf(), for facts without files too. */
export const fileIn = (
  d: ReelData | null,
  id: CaptureId,
  path: string,
): string | null => d?.files?.[`${id}/${path}`] ?? null;

/** Up to the next 1/16 beat (ART.md §13's grid): where a move that follows a take's time starts. */
export const onGrid = (b: number): number => Math.ceil(b * 16 - 1e-6) / 16;

/** What a take typed for step `name`: its keys up to its Enter, or null without the take. */
export function typedOf(c: Capture | null, name: string): string | null {
  if (!c) return null;
  const s = stepOf(c, name);
  return c.keys
    .filter(
      ([t, k]) => t > s.start && t < s.enter && k.length === 1 && k >= " ",
    )
    .map(([, k]) => k)
    .join("");
}

/**
 * The take time the whole of step `name`'s command is on screen, just
 * before its Enter: the frame after its last typed key.
 */
export function typedFrame(c: Capture, name: string): number {
  const s = stepOf(c, name);
  const keys = c.keys.filter(
    ([t, k]) => t > s.start && t < s.enter && k !== "\r",
  );
  const last = keys.length ? keys[keys.length - 1][0] : s.type;
  return frameAfter(c, last);
}

/**
 * An install whose command kinetic type stood in for (the take already
 * cut to it typed, typedFrame): from beat `at`, a beat of anticipation on
 * the typed command (ENTER_PAUSE), Enter, and its rows at LAPSE × up to
 * the frame before a summary (CUT_ON); the shot's tail fades them out and
 * holds the frame the command was entered on. For Pace.lapse, whose rows
 * start on the second play.
 */
export function enteredInstall(c: Capture, name: string, at: number): Play[] {
  const u = stepOf(c, name);
  const t = typed(
    at,
    Math.max(typedFrame(c, name), u.enter - 0.05),
    u.enter,
    u.enter + 0.02,
  );
  return [
    t,
    play(
      at + playLen(t),
      u.enter + 0.02,
      lastBefore(c, CUT_ON, u.enter, u.end),
      LAPSE,
    ),
  ];
}

/**
 * The beat the first word of section `id`'s captions matching `re` (its
 * backticks dropped) lands: words land one per 1/32 note, the line's last
 * on its `in` (type.ts drawWords); with `events`, the captions as a scene
 * that reports them anchors them. Null when no caption has it.
 */
export function wordLands(
  id: SectionId,
  d: ReelData | null,
  re: RegExp,
  events?: SceneEvents | null,
): number | null {
  for (const cap of captionsFor(id, d, events))
    for (const line of cap.lines) {
      const words = line.text.split(" ").filter(Boolean);
      const i = words.findIndex((w) => re.test(w.replaceAll("`", "")));
      if (i >= 0) return line.in - ((words.length - 1 - i) * WORD) / BEAT;
    }
  return null;
}

// Kinetic type (ART.md §12.10): use's slam and backends' `npm:` both stand
// under the short pane (PANES.slip) in the left column, and shrink into
// the command's cells on its prompt row.

/** Kinetic type's centre x and baseline, under PANES.slip (y 120–450) and clear of the caption band. */
export const KINETIC = {
  x: LAYOUT.bigX,
  y: PANES.slip.y + PANES.slip.h + 150,
} as const;

/**
 * How far through its shrink (0 to 1, of DUR.bigShrink) kinetic type
 * reaches its cells: it moves and shrinks on `move` until then, opaque,
 * and lands exactly on them (size, x and baseline); the take's frame with
 * the word typed cuts in under it on that beat (wordFrame), and the type
 * fades out over the rest of the shrink, so the landing hands over glyph
 * for glyph instead of one line settling through another.
 */
export const ARRIVE_BY = 0.85;

/**
 * Kinetic type's shrink at beat `b`, from `from` (its start): `k`, how far
 * it has travelled (on `move`, 1 once it has arrived at ARRIVE_BY), and
 * its opacity and colour, mixing from `color` to the terminal's text by
 * the arrival and fading out after it.
 */
export function shrinkAt(
  b: number,
  from: number,
  color: string,
): { k: number; alpha: number; color: string } {
  const u = (b - from) / DUR.bigShrink;
  return {
    k: span(b, from, ARRIVE_BY * DUR.bigShrink, "move"),
    alpha: 1 - smoothstep(ARRIVE_BY, 1, u),
    color: mix(color, TERM.text, smoothstep(0.1, ARRIVE_BY, u)),
  };
}

/**
 * The take time step `name`'s command shows typed up to (not including)
 * `word`, its end: the frame after the key before the word's first
 * character, or the step's first frame when the word is the whole
 * command. Kinetic type stands in for the word: the take plays its
 * typing to here, then cuts to typedFrame as the type lands.
 */
export function wordFrame(c: Capture, name: string, word: string): number {
  const s = stepOf(c, name);
  const keys = c.keys.filter(
    ([t, k]) => t > s.start && t < s.enter && k.length === 1 && k >= " ",
  );
  const cmd = keys.map(([, k]) => k).join("");
  const at = Array.from(
    cmd.slice(0, Math.max(0, cmd.lastIndexOf(word))),
  ).length;
  return at > 0 ? frameAfter(c, keys[at - 1][0]) : s.type;
}

// Lifting a value (ART.md §9).

/** How long a lift takes to land, beats: the chip forming, the rise, the flight. */
export const LIFT_LANDS = DUR.flick + DUR.flick + DUR.lift;

/** How far over full size a landing chip pops, and back (a snap's feel, without moving it off its row). */
const LAND_POP = 0.06;

/**
 * How far above its card row a lifted chip comes down from, px (the
 * flight's second control point, over the landing spot): it drops onto
 * its row the way a seated line does, rather than sliding along it.
 */
const DROP_FROM = 72;
/** How far along the throw the chip keeps level with its own row, which is empty right of the value. */
const LEVEL_FOR = 0.45;
/**
 * How far through its flight the chip starts growing to TYPE.chip.lift:
 * grown on its baseline, it rises into the row above, so it waits until
 * it has run past that row's text (past x ≈ 780 for the pitch's rows).
 */
const GROW_FROM = 0.4;

export interface Lift {
  /** The captured value's runs, as the pane shows them. */
  runs: readonly Run[];
  /** Where the value stands in the pane: column 0's left edge, its baseline, its size. */
  from: { x: number; y: number; size: number };
  /** Where the chip lands: its left edge and its text's baseline. */
  to: { x: number; y: number };
  /** The beat the chip starts to form. */
  at: number;
  /** The beat the chip and its thread start to leave. */
  out: number;
  /**
   * The thread, drawn on from the landing: its corners, from the value's
   * right edge to the card row's anchor. None drawn without.
   */
  route?: readonly Pt[];
}

/**
 * Where a lifted chip is `u` of the way along its flight (0 to 1): an arc
 * (a cubic) that leaves level along the value's own row (empty to the
 * right of the value, so the chip crosses no pane text on its way out),
 * lifts over the gutter, and comes down onto the landing spot from
 * DROP_FROM px above it, so its last stretch is a drop onto its card row
 * rather than a slide along the row's text. Its highest point is over the
 * card, under the card's own tab strip for its first rows.
 */
function flightAt(a: Pt, z: Pt, u: number): Pt {
  const c1 = { x: a.x + LEVEL_FOR * (z.x - a.x), y: a.y };
  const c2 = { x: z.x, y: z.y - DROP_FROM };
  const v = 1 - u;
  const k0 = v * v * v;
  const k1 = 3 * v * v * u;
  const k2 = 3 * v * u * u;
  const k3 = u * u * u;
  return {
    x: k0 * a.x + k1 * c1.x + k2 * c2.x + k3 * z.x,
    y: k0 * a.y + k1 * c1.y + k2 * c2.y + k3 * z.y,
  };
}

/**
 * A value chip: a TERM.window pill round `text` (its left edge `x`, the
 * text's baseline `y`, mono `size`), its fill at `fill`, its outline at
 * `edge`, and its shadow `air` of the way from the resting `small` to the
 * in-flight `float` (ART.md §5). Returns its rect.
 */
function liftChip(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  size: number,
  o: { fill: number; edge: number; air: number; color?: string },
): { x: number; y: number; w: number; h: number } {
  const w = Array.from(text).length * advance(size) + size;
  const h = Math.round(size * CHIP_ART.heightEm);
  const r = { x, y: y - h * 0.72, w, h };
  const path = () => roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.chip);
  ctx.save();
  ctx.globalAlpha *= o.fill;
  castShadow(ctx, SHADOW.small, path, 1 - o.air, r);
  castShadow(ctx, SHADOW.float, path, o.air, r);
  path();
  ctx.fillStyle = TERM.window;
  ctx.fill();
  ctx.restore();
  if (o.edge > 0) {
    ctx.save();
    ctx.globalAlpha *= o.fill * o.edge;
    roundedRect(ctx, r.x + 1, r.y + 1, r.w - 2, r.h - 2, RADIUS.chip - 1);
    ctx.lineWidth = STROKE.badge;
    ctx.strokeStyle = COLOR.mark;
    ctx.stroke();
    ctx.restore();
  }
  ctx.save();
  ctx.globalAlpha *= o.fill;
  drawTermLine(ctx, [run(text, o.color ?? TERM.text)], x + size / 2, y, size);
  ctx.restore();
  return r;
}

/**
 * A value lifting out of a terminal row (ART.md §9, form, then rise): a
 * chip forms round a copy of the captured run in place over 1/8 beat, its
 * fill covering the pane's own glyphs from the first frame (so the row
 * never reads doubled); it rises 6 px over the next 1/8 with its shadow
 * lifting to `float`; it flies an arc (flightAt, travel) over 3/4 beat to
 * its card row, growing from the pane's size to
 * TYPE.chip.lift (40, the size a lifted value reads at on a phone), and
 * lands with a snap over 1/4, its shadow settling back to `small`; then
 * its thread draws on over 1/2 beat. From `out` the chip fades over
 * DUR.exit (glide) and the thread over DUR.threadFade, so both are gone by
 * out + 1/2.
 */
export function drawLift(
  ctx: CanvasRenderingContext2D,
  l: Lift,
  b: number,
): void {
  const t = b - l.at;
  if (t < 0 || b >= l.out + DUR.threadFade) return;
  const text = l.runs.map((r) => r.text).join("");
  const color = l.runs[0]?.color;
  const leave = 1 - span(b, l.out, DUR.exit, "glide");
  const form = span(b, l.at, DUR.flick, "arrive");
  const rise = span(b, l.at + DUR.flick, DUR.flick, "arrive");
  const u = span(b, l.at + 2 * DUR.flick, DUR.lift, "travel");
  const landed = l.at + LIFT_LANDS;
  // The thread under the chip: out of the value, along the gutter, into the card.
  if (l.route && b >= landed) {
    const k = span(b, landed, DUR.thread, "arrive");
    const fade = 1 - span(b, l.out, DUR.threadFade, "glide");
    threadAlong(ctx, l.route, k, fade);
  }
  if (leave <= 0) return;
  const size0 = l.from.size;
  const size1 = TYPE.chip.lift;
  // The chip's text sits size/2 in from its left edge.
  const start: Pt = {
    x: l.from.x - size0 / 2,
    y: l.from.y - CHIP_ART.liftRise * rise,
  };
  const end: Pt = { x: l.to.x, y: l.to.y };
  const p = u >= 1 ? end : flightAt(start, end, u);
  // It keeps the pane's size (a row tall) while it runs along its own row,
  // and grows once it is past the text of the row above.
  const size = lerp(size0, size1, smoothstep(GROW_FROM, 1, u));
  // The landing's snap: a touch over full size, and back, over 1/4 beat.
  const pop = 1 + LAND_POP * bump(b, landed, DUR.tick);
  // In the air from the rise until the landing settles.
  const air = rise * (1 - span(b, landed, DUR.tick, "arrive"));
  const w = Array.from(text).length * advance(size) + size;
  ctx.save();
  ctx.globalAlpha *= leave;
  ctx.translate(p.x + w / 2, p.y);
  ctx.scale(pop, pop);
  ctx.translate(-(p.x + w / 2), -p.y);
  liftChip(ctx, text, p.x, p.y, size, { fill: 1, edge: form, air, color });
  ctx.restore();
}

/**
 * A thread's route from a value in a pane to a card row (ART.md §9): out
 * of the value's right edge + 12, along its row's middle to the gutter,
 * up or down the gutter, and into the card's left edge, ending on the
 * row's anchor (card x + 8). Straight when the two rows are level; when
 * they are nearly level (closer than two corner radii, where a vertical
 * gutter run would draw an S-kink), it changes lanes on one slanted run
 * across the gutter instead.
 */
export function routeInto(
  from: { x: number; y: number },
  to: { x: number; y: number },
): Pt[] {
  const x0 = from.x + THREAD.exit;
  const dy = Math.abs(to.y - from.y);
  if (dy < 4)
    return [
      { x: x0, y: from.y },
      { x: to.x, y: from.y },
    ];
  if (dy < 2 * THREAD.corner) {
    const half = Math.min(3 * THREAD.corner, (to.x - x0) / 4);
    return [
      { x: x0, y: from.y },
      { x: THREAD.gutterX - half, y: from.y },
      { x: THREAD.gutterX + half, y: to.y },
      { x: to.x, y: to.y },
    ];
  }
  return [
    { x: x0, y: from.y },
    { x: THREAD.gutterX, y: from.y },
    { x: THREAD.gutterX, y: to.y },
    { x: to.x, y: to.y },
  ];
}

/** A shot's screen and layout, as kit/grey.ts shot returns them. */
interface Shown {
  screen: { lines: readonly (readonly Run[])[] };
  layout: {
    col(c: number): number;
    baseline(i: number): number;
    advance: number;
  };
  pane: { size: number };
}

/**
 * Where `word` (the end of `cmd`, the command the take typed) stands on the
 * screen's last prompt row: its cells' centre, their baseline and the
 * pane's size, for kinetic type to shrink into. Null when no prompt shows.
 */
export function cellsOf(
  s: Shown | null,
  cmd: string,
  word: string,
): { x: number; y: number; size: number } | null {
  if (!s) return null;
  const texts = s.screen.lines.map((l) => l.map((r) => r.text).join(""));
  for (let i = texts.length - 1; i >= 0; i--) {
    // A bare prompt too (its trailing space trimmed), before anything is typed.
    const m = /^(~\S* \$)(?: |$)/.exec(texts[i]);
    if (!m) continue;
    const col = m[1].length + 1 + cmd.lastIndexOf(word);
    const n = Array.from(word).length;
    return {
      x: s.layout.col(col) + (n * s.layout.advance) / 2,
      y: s.layout.baseline(i),
      size: s.pane.size,
    };
  }
  return null;
}
