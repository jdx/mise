// from jdx/hk@37937824 docs/.vitepress/theme/showreel/kit/term.ts
// A terminal. A window (chrome bar, three dots, a hairline edge) and a fixed
// character grid: every character sits at x0 + col × advance, with the
// advance 0.6 of the size (JetBrains Mono's own), and nothing is measured,
// so the glyphs JetBrains Mono lacks (✔, ℹ, ↳, the braille spinner and the
// block elements) are drawn as vectors in their cells and can never shift a
// column.
//
// A line is plain text, coloured by the default rules (the shell's prompt),
// or runs a capture has already coloured, or text with the ANSI SGR codes
// the capture recorded (sgrRuns). The spinner and the cursor run on GLOBAL
// time, so a pane that crosses a bar line keeps both in phase.
//
// Adapted for mise: hk's header and output rules are gone; the colours are
// mise's hero terminal (bible.ts TERM), and captured ANSI stays true to the
// capture. The window is a raised object on the stage (ART.md §6): a 56 px
// chrome bar with the cwd of the latest prompt as its title, a cwd dot, and
// the badges a shot carries in its right end; a cast shadow; block elements
// filling their whole cells, so a block drawing has no seams between rows.

import { type LitRect, PALETTE, TERM } from "../bible";
import { mix, rgba } from "../color";
import { castShadows, roundedRect } from "../fx";
import { clamp, lerp, smoothstep, TAU } from "../math";
import { drawText, font, layout, MONO } from "../type";
import { ink } from "./ink";
import { cursorOn, type Rect } from "./motion";
import {
  BADGE,
  CHROME as CHROME_ART,
  CHROME_H,
  COLOR,
  PANE_DIM,
  PANES,
  RADIUS as RADII,
  SHADOW,
  STROKE,
  TYPE,
} from "./style";

/** The chrome bar's height, px (kit/style.ts CHROME_H). */
export const CHROME = CHROME_H;
/** The window's corner radius, px. */
export const RADIUS = RADII.window;

/** A character cell's width at `size` px: JetBrains Mono's advance, 0.6 em. */
export const advance = (size: number): number => 0.6 * size;

/** Where a terminal stands and how its text is set. */
export interface Pane extends Rect {
  /** A chrome bar with three dots across the top. */
  chrome: boolean;
  /** Paint the window; off for text set in a panel the scene draws. */
  window: boolean;
  /** Mono px, and baseline to baseline. */
  size: number;
  lineH: number;
  /** Column 0's left edge, and row 0's baseline. */
  x0: number;
  baseline0: number;
  /** Rows the pane holds. */
  rows: number;
  /** Width of the fade that cuts lines off at the window's right edge, px; 0 where 80 columns fit. */
  fade: number;
  /**
   * The rows a screen taller than the pane shows: the bottom ones, as a
   * terminal scrolls, or the top ones (`{ ...PANE_FULL, anchor: "top" }`).
   */
  anchor: "bottom" | "top";
  /**
   * A fitted pane growing to its next row (kit/rest.ts fittedPane): the
   * rows its screen scrolls by already, while its bottom edge catches up,
   * so the text holds still and the new row shows as the edge clears it.
   */
  fitRows?: number;
}

/** The full terminal: 80 columns and 11 rows of 32 px mono across the stage (kit/style.ts PANES.solo). */
export const PANE_FULL: Pane = PANES.solo;

/** The terminal cut down to its last two rows above a scene. */
export const STRIP: Pane = {
  ...PANE_FULL,
  h: CHROME_H + 16 + 2 * PANE_FULL.lineH + 14,
  rows: 2,
};

/**
 * A pane `k` of the way from `a` to `b` (PANE_FULL squeezing into STRIP).
 * The numbers tween; the switches flip halfway. Pass a scroll to drawTerm
 * as well, so lines leave through the top as the pane shrinks.
 */
export function lerpPane(a: Pane, b: Pane, k: number): Pane {
  const n = (
    key:
      | "x"
      | "y"
      | "w"
      | "h"
      | "size"
      | "lineH"
      | "x0"
      | "baseline0"
      | "rows"
      | "fade",
  ) => lerp(a[key], b[key], k);
  const s = k < 0.5 ? a : b;
  return {
    x: n("x"),
    y: n("y"),
    w: n("w"),
    h: n("h"),
    chrome: s.chrome,
    window: s.window,
    size: n("size"),
    lineH: n("lineH"),
    x0: n("x0"),
    baseline0: n("baseline0"),
    rows: n("rows"),
    fade: n("fade"),
    anchor: s.anchor,
  };
}

/** The fewest rows a fitted pane holds (fitPane): a short screen still reads as a terminal, not a strip. */
export const FIT_MIN = 6;

/** The height pane `p` needs to hold `n` rows: its chrome, its first baseline's inset, n rows and its bottom pad. */
export function paneHeight(p: Pane, n: number): number {
  const top0 = p.baseline0 - 0.8 * p.size - (p.lineH - p.size) / 2;
  return Math.ceil(top0 - p.y + n * p.lineH + 14);
}

/**
 * Pane `p` cut down to hold `n` rows (at least `min`, FIT_MIN by default,
 * and never more than its own): the same top-left corner, columns and
 * type, a shorter window. For a screen that shows a few rows in a tall
 * pane (the lead's "mostly empty panes"): size the preset to its content.
 * A pane growing as output needs rows tweens between two fits with
 * lerpPane on EASE.move (or kit/rest.ts fittedPane does it from a take).
 */
export function fitPane(p: Pane, n: number, min: number = FIT_MIN): Pane {
  const rows = Math.max(Math.min(min, p.rows), Math.min(p.rows, Math.ceil(n)));
  return { ...p, h: Math.min(p.h, paneHeight(p, rows)), rows };
}

/**
 * Pane `p` moved so its top-left corner is at `at` (either coordinate),
 * its text with it: for centring a fitted pane in the actor band
 * (centreIn), or placing one off the grid's corner (the open).
 */
export function placePane(p: Pane, at: { x?: number; y?: number }): Pane {
  const dx = (at.x ?? p.x) - p.x;
  const dy = (at.y ?? p.y) - p.y;
  return {
    ...p,
    x: p.x + dx,
    y: p.y + dy,
    x0: p.x0 + dx,
    baseline0: p.baseline0 + dy,
  };
}

/** Pane `p` centred vertically in `band` (the actor band, kit/style.ts LAYOUT.left by default: y 120 to 690). */
export const centreIn = (p: Pane, band: Rect): Pane =>
  placePane(p, { y: Math.round(band.y + (band.h - p.h) / 2) });

/** The window's body: under the chrome bar, if there is one. */
export const bodyOf = (p: Pane): Rect => {
  const top = p.chrome ? Math.min(CHROME, p.h) : 0;
  return { x: p.x, y: p.y + top, w: p.w, h: p.h - top };
};

/**
 * The first of `n` lines a pane shows: 0 while they fit, then the bottom
 * rows (a terminal scrolls), or always 0 for a top-anchored pane.
 */
export function firstLine(p: Pane, n: number): number {
  return p.anchor === "top"
    ? 0
    : Math.max(0, n - Math.floor(paneRows(p) + 1e-9));
}

/**
 * The rows a pane shows: its `rows`, or as many as its window has room
 * for when that is fewer (a pane shortened without recounting its rows),
 * so a terminal never sets a row below its window: it scrolls instead.
 */
export function paneRows(p: Pane): number {
  const top0 = p.baseline0 - 0.8 * p.size - (p.lineH - p.size) / 2;
  const fit = Math.floor((p.y + p.h - 12 - top0) / p.lineH + 1e-9);
  return Math.max(0, Math.min(p.rows, fit));
}

/** Where a screen's lines and cells are in a pane. */
export interface TermLayout {
  /** The first line shown; fractional while a scene scrolls. */
  first: number;
  /** Column width. */
  advance: number;
  /** Line i's baseline (i indexes the screen's lines, not the pane's rows). */
  baseline(i: number): number;
  /** Column c's left edge. */
  col(c: number): number;
  /**
   * Line i's column c as a terminal cell: a line's height, the text's em box
   * centred in it. The cursor fills it; a glyph's centre is its middle.
   */
  cell(i: number, c: number): Rect;
  /** The window's body, where the text is clipped. */
  body: Rect;
}

/** The layout of an `n`-line screen in `p`, scrolled to `scroll` (default firstLine). */
export function termLayout(p: Pane, n: number, scroll?: number): TermLayout {
  const first = scroll ?? firstLine(p, n);
  const adv = advance(p.size);
  const baseline = (i: number) => p.baseline0 + (i - first) * p.lineH;
  const col = (c: number) => p.x0 + c * adv;
  return {
    first,
    advance: adv,
    baseline,
    col,
    cell: (i, c) => ({
      x: col(c),
      y: baseline(i) - 0.8 * p.size - (p.lineH - p.size) / 2,
      w: adv,
      h: p.lineH,
    }),
    body: bodyOf(p),
  };
}

// Colours.

/** A stretch of a line in one colour and weight. */
export interface Run {
  text: string;
  color: string;
  bold: boolean;
  /**
   * The cells' background, where the capture set one (captures.ts bgOf:
   * the highlighted choice in a confirm prompt, `  Yes  ` on pink);
   * absent for the terminal's own.
   */
  bg?: string;
}

/** A terminal line: text coloured by styleLine, or runs already coloured. */
export type TermLine = string | readonly Run[];

export const run = (
  text: string,
  color: string = TERM.text,
  bold = false,
  bg?: string,
): Run =>
  bg === undefined ? { text, color, bold } : { text, color, bold, bg };

/** A shell prompt, as the capture rig's PS1 prints it: `~/work/api $ ` or `$ `. */
const PROMPT = /^(\S* ?\$ )(.*)$/u;

/**
 * A plain line's colours, from its text alone: the shell's prompt (up to
 * and including `$ `) in the prompt colour, the rest in the default.
 * Anything a capture coloured arrives as runs or SGR codes instead.
 */
export function styleLine(text: string): Run[] {
  if (text.includes("\x1b")) return sgrRuns(text);
  const m = PROMPT.exec(text);
  if (m) return [run(m[1], TERM.prompt), run(m[2])].filter((r) => r.text);
  return [run(text)];
}

/** A line's runs: as given, or styled from its text. */
export const runsOf = (line: TermLine): readonly Run[] =>
  typeof line === "string" ? styleLine(line) : line;

/** A line's characters, as the grid sets them: one per column. */
export const lineText = (line: TermLine): string =>
  runsOf(line)
    .map((r) => r.text)
    .join("");

/**
 * Text with ANSI SGR codes (ESC [ … m) as runs: 30–37 and 90–97 set the
 * foreground from TERM.ansi, 39 and 0 reset it, 1 is bold, 2 is dim, 22
 * clears both; 40–47 and 100–107 set the background from TERM.ansi, 49
 * and 0 reset it. Other SGR codes (256-colour and RGB colours, underline)
 * are dropped, and so is every other escape.
 */
export function sgrRuns(text: string): Run[] {
  const out: Run[] = [];
  let color: string = TERM.text;
  let bg: string | undefined;
  let bold = false;
  let dim = false;
  let buf = "";
  const flush = () => {
    if (buf) out.push(run(buf, dim ? TERM.dim : color, bold, bg));
    buf = "";
  };
  for (let i = 0; i < text.length;) {
    if (text[i] === "\x1b") {
      const rest = text.slice(i);
      // CSI: parameter bytes, intermediates, then one final byte.
      const m = /^\x1b\[([0-?]*)([ -/]*)([@-~])/.exec(rest);
      if (!m) {
        // OSC (a title or a link) up to BEL or ST, a charset (ESC ( B), or
        // any other two-byte escape.
        const other =
          /^\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|^\x1b[()*+][ -~]|^\x1b[ -~]?/.exec(
            rest,
          );
        i += other ? other[0].length : 1;
        continue;
      }
      if (m[3] === "m" && /^[0-9;:]*$/.test(m[1])) {
        flush();
        const codes = m[1] === "" ? [0] : m[1].split(/[;:]/).map(Number);
        for (let k = 0; k < codes.length; k++) {
          const c = codes[k];
          if (c === 0) {
            [color, bold, dim] = [TERM.text, false, false];
            bg = undefined;
          } else if (c === 1) bold = true;
          else if (c === 2) dim = true;
          else if (c === 22) [bold, dim] = [false, false];
          else if (c === 39) color = TERM.text;
          else if (c === 49) bg = undefined;
          else if (c >= 30 && c <= 37) color = TERM.ansi[c - 30];
          else if (c >= 90 && c <= 97) color = TERM.ansi[c - 90 + 8];
          else if (c >= 40 && c <= 47) bg = TERM.ansi[c - 40];
          else if (c >= 100 && c <= 107) bg = TERM.ansi[c - 100 + 8];
          // 38;5;n and 38;2;r;g;b carry their own arguments: skip past them.
          else if (c === 38 || c === 48)
            k += codes[k + 1] === 5 ? 2 : codes[k + 1] === 2 ? 4 : 0;
        }
      }
      i += m[0].length;
      continue;
    }
    buf += text[i++];
  }
  flush();
  return out;
}

// The glyphs JetBrains Mono lacks, drawn in their cells.

/** The braille spinner, one frame per 200 ms of real time, as a terminal turns it. */
export const SPINNER = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

/** The spinner's frame at GLOBAL time `t`. */
export const spinnerFrame = (t: number): string =>
  SPINNER[Math.floor(t / 0.2 + 1e-9) % SPINNER.length];

/** A block element (U+2580–U+259F): drawn as rectangles filling its cell. */
export const isBlockGlyph = (ch: string): boolean => ch >= "▀" && ch <= "▟";

/** Characters term.ts draws itself: ✔, ℹ, ↳, all of braille and the block elements. */
export const isVectorGlyph = (ch: string): boolean =>
  ch === "✔" ||
  ch === "ℹ" ||
  ch === "↳" ||
  (ch >= "⠀" && ch <= "⣿") ||
  isBlockGlyph(ch);

/** A rect of the unit cell: x, y (down from the cell's top), w, h, each 0 to 1. */
export type CellRect = readonly [x: number, y: number, w: number, h: number];

const UL: CellRect = [0, 0, 0.5, 0.5];
const UR: CellRect = [0.5, 0, 0.5, 0.5];
const LL: CellRect = [0, 0.5, 0.5, 0.5];
const LR: CellRect = [0.5, 0.5, 0.5, 0.5];
/** The quadrant blocks, U+2596 to U+259F, in code point order. */
const QUADRANTS: readonly (readonly CellRect[])[] = [
  [LL],
  [LR],
  [UL],
  [UL, LL, LR],
  [UL, LR],
  [UL, UR, LL],
  [UL, UR, LR],
  [UR],
  [UR, LL],
  [UR, LL, LR],
];

/**
 * A block element's shape in its cell: the rects it fills (of the whole
 * cell, the advance wide and the line pitch tall) and the ink's coverage
 * (the shades ░ ▒ ▓ fill the cell at a quarter, half and three quarters).
 * Null for anything else.
 */
export function blockRects(
  ch: string,
): { rects: readonly CellRect[]; alpha: number } | null {
  if (!isBlockGlyph(ch)) return null;
  const cp = ch.codePointAt(0)!;
  const one = (r: CellRect) => ({ rects: [r], alpha: 1 });
  if (cp === 0x2580) return one([0, 0, 1, 0.5]);
  // Lower one eighth to seven eighths, then the full block.
  if (cp <= 0x2588) {
    const n = (cp - 0x2580) / 8;
    return one([0, 1 - n, 1, n]);
  }
  // Left seven eighths down to one eighth.
  if (cp <= 0x258f) {
    const n = (0x2590 - cp) / 8;
    return one([0, 0, n, 1]);
  }
  if (cp === 0x2590) return one([0.5, 0, 0.5, 1]);
  if (cp <= 0x2593) return { rects: [[0, 0, 1, 1]], alpha: (cp - 0x2590) / 4 };
  if (cp === 0x2594) return one([0, 0, 1, 1 / 8]);
  if (cp === 0x2595) return one([7 / 8, 0, 1 / 8, 1]);
  return { rects: QUADRANTS[cp - 0x2596], alpha: 1 };
}

/** How far a block's rects reach past their cell on every side, px: no seam between cells. */
const OVERSCAN = 0.5;

/** JetBrains Mono's cap height, em. */
const CAP = 0.73;

/**
 * Draw a vector glyph in the cell whose left edge is `x`, on baseline `y`,
 * at `size` px, in `paint`. Each is sized to JetBrains Mono's cap height and
 * weight, so it reads as the font's own.
 */
function drawGlyph(
  ctx: CanvasRenderingContext2D,
  ch: string,
  x: number,
  y: number,
  size: number,
  paint: string | CanvasGradient,
): void {
  const w = advance(size);
  const u = size;
  const k = CAP / 0.66;
  ctx.save();
  ctx.strokeStyle = paint;
  ctx.fillStyle = paint;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  if (ch === "✔") {
    // Heavy check mark: a short down-stroke into a long rising one, a
    // little taller than the x-height.
    ctx.lineWidth = 0.14 * u;
    ctx.moveTo(x + 0.12 * w, y - 0.3 * k * u);
    ctx.lineTo(x + 0.4 * w, y - 0.06 * u);
    ctx.lineTo(x + 0.9 * w, y - 0.6 * k * u);
    ctx.stroke();
  } else if (ch === "ℹ") {
    // Information source: a dot over a stem with a foot, cap height tall.
    ctx.lineWidth = 0.1 * u;
    ctx.moveTo(x + 0.36 * w, y - 0.44 * u);
    ctx.lineTo(x + 0.52 * w, y - 0.44 * u);
    ctx.lineTo(x + 0.52 * w, y);
    ctx.moveTo(x + 0.3 * w, y);
    ctx.lineTo(x + 0.74 * w, y);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(x + 0.52 * w, y - CAP * u + 0.06 * u, 0.065 * u, 0, TAU);
    ctx.fill();
  } else if (ch === "↳") {
    // Downwards arrow with tip rightwards: down from cap height, then right.
    ctx.lineWidth = 0.075 * u;
    ctx.moveTo(x + 0.22 * w, y - CAP * u);
    ctx.lineTo(x + 0.22 * w, y - 0.22 * u);
    ctx.lineTo(x + 0.9 * w, y - 0.22 * u);
    ctx.moveTo(x + 0.66 * w, y - 0.4 * u);
    ctx.lineTo(x + 0.9 * w, y - 0.22 * u);
    ctx.lineTo(x + 0.66 * w, y - 0.04 * u);
    ctx.stroke();
  } else {
    // Braille: a 2×4 matrix of dots; bit n of the code point after U+2800
    // is dot n + 1 (1–3 down the left, 4–6 down the right, then 7 and 8 in
    // the descender). Dots 1–6, all the spinner uses, span cap height to
    // baseline, as a font's braille does.
    const bits = ch.codePointAt(0)! - 0x2800;
    const r = 0.09 * k * u;
    const cx = [x + 0.5 * w - 0.13 * u, x + 0.5 * w + 0.13 * u];
    const cy = (row: number) => y - 0.57 * k * u + 0.24 * k * u * row;
    const DOTS: readonly [col: number, row: number][] = [
      [0, 0],
      [0, 1],
      [0, 2],
      [1, 0],
      [1, 1],
      [1, 2],
      [0, 3],
      [1, 3],
    ];
    DOTS.forEach(([c, row], n) => {
      if (!(bits & (1 << n))) return;
      ctx.moveTo(cx[c] + r, cy(row));
      ctx.arc(cx[c], cy(row), r, 0, TAU);
    });
    ctx.fill();
  }
  ctx.restore();
}

// Drawing.

export interface LineOptions {
  /** GLOBAL seconds (env.t): the spinner's frame. Without it a spinner keeps the frame in the text. */
  t?: number;
  /** Fade the text out from x0 to x1 (a pane's right edge), px. */
  fade?: readonly [x0: number, x1: number] | null;
  /**
   * The line pitch, px: how tall a block element's cell is (a terminal
   * cell is a line tall). Defaults to 1.375 em, the solo pane's 44 at 32.
   */
  lineH?: number;
}

/**
 * Where a block's cell edges fall: on whole device pixels when the
 * transform does not rotate (so neighbouring cells and rows meet with no
 * seam and no overlap, at any alpha), else pushed OVERSCAN past the cell.
 */
function blockEdges(
  ctx: CanvasRenderingContext2D,
): (
  x0: number,
  y0: number,
  x1: number,
  y1: number,
) => [number, number, number, number] {
  const m = ctx.getTransform();
  if (Math.abs(m.b) < 1e-9 && Math.abs(m.c) < 1e-9 && m.a > 0 && m.d > 0) {
    const sx = (x: number) => (Math.round(x * m.a + m.e) - m.e) / m.a;
    const sy = (y: number) => (Math.round(y * m.d + m.f) - m.f) / m.d;
    return (x0, y0, x1, y1) => {
      const [a, b, c, d] = [sx(x0), sy(y0), sx(x1), sy(y1)];
      return [a, b, c - a, d - b];
    };
  }
  return (x0, y0, x1, y1) => [
    x0 - OVERSCAN,
    y0 - OVERSCAN,
    x1 - x0 + 2 * OVERSCAN,
    y1 - y0 + 2 * OVERSCAN,
  ];
}

/**
 * One terminal line on the grid: column 0's left edge at `x0`, on baseline
 * `y`, at `size` px. For a line a scene moves by itself; drawTerm draws a
 * pane's lines with it. Block elements fill their whole cells: `lineH`
 * tall, centred on the text's em box as the cursor is.
 */
export function drawTermLine(
  ctx: CanvasRenderingContext2D,
  line: TermLine,
  x0: number,
  y: number,
  size: number,
  o: LineOptions = {},
): void {
  const adv = advance(size);
  const lineH = o.lineH ?? size * 1.375;
  const top = y - 0.8 * size - (lineH - size) / 2;
  ink(lineText(line), "term");
  // A fade is each run's colour turning transparent across the fade, so it
  // stays exact under any alpha the caller has set.
  // A fade under a pixel wide is none: a pane tweening between a faded
  // preset and an unfaded one (lerpPane) passes through fades a hair wide,
  // and Chromium paints nothing with a gradient that degenerate.
  const paint = (color: string): string | CanvasGradient => {
    if (!o.fade || o.fade[1] - o.fade[0] < 1) return color;
    const g = ctx.createLinearGradient(o.fade[0], 0, o.fade[1], 0);
    g.addColorStop(0, color);
    g.addColorStop(1, rgba(color, 0));
    return g;
  };
  let edges: ReturnType<typeof blockEdges> | null = null;
  ctx.save();
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  let col = 0;
  for (const r of runsOf(line)) {
    const fill = paint(r.color);
    // A run's own background fills its cells, a line tall, under its text.
    if (r.bg) {
      const n = Array.from(r.text).length;
      edges ??= blockEdges(ctx);
      const [bx, by, bw, bh] = edges(
        x0 + col * adv,
        top,
        x0 + (col + n) * adv,
        top + lineH,
      );
      ctx.fillStyle = paint(r.bg);
      ctx.fillRect(bx, by, bw, bh);
    }
    ctx.font = font(size, r.bold ? 700 : 400, MONO);
    ctx.fillStyle = fill;
    // A run's blocks, by coverage, each filled as one path, so the cells
    // of a block drawing are one shape.
    let blocks: Map<number, [number, number, number, number][]> | null = null;
    // One character per column, never a run set whole: Chromium rounds a
    // run's advances to whole pixels (19 px, not 19.2, at 32 px), which
    // walks a long run off its columns.
    for (const ch of r.text) {
      const x = x0 + col * adv;
      const blk = blockRects(ch);
      if (blk) {
        edges ??= blockEdges(ctx);
        blocks ??= new Map();
        let list = blocks.get(blk.alpha);
        if (!list) blocks.set(blk.alpha, (list = []));
        for (const [bx, by, bw, bh] of blk.rects)
          list.push(
            edges(
              x + bx * adv,
              top + by * lineH,
              x + (bx + bw) * adv,
              top + (by + bh) * lineH,
            ),
          );
      } else if (isVectorGlyph(ch))
        drawGlyph(
          ctx,
          o.t !== undefined && SPINNER.includes(ch) ? spinnerFrame(o.t) : ch,
          x,
          y,
          size,
          fill,
        );
      else if (ch !== " ") ctx.fillText(ch, x, y);
      col++;
    }
    if (blocks)
      for (const [alpha, rects] of blocks) {
        ctx.save();
        ctx.globalAlpha *= alpha;
        ctx.fillStyle = fill;
        ctx.beginPath();
        for (const q of rects) ctx.rect(q[0], q[1], q[2], q[3]);
        ctx.fill();
        ctx.restore();
      }
  }
  ctx.restore();
}

// The window.

export interface WindowOptions {
  /** 0 to 1: how far the window has dimmed (PANE_DIM): its body mixing toward the stage. */
  dim?: number;
  /** Cast its shadow on the stage (default true). */
  shadow?: boolean;
}

/**
 * A terminal window at `r` (ART.md §6): a raised object with its cast
 * shadow, the body, the 56 px chrome bar with three neutral dots and a rule
 * under it if `chrome`, a highlight inside the top edge, and the 1 px edge,
 * drawn inside the rect so a lit rect of it covers it exactly. The title and
 * badges in the bar are drawn with the text (drawTerm, drawChromeLabels).
 */
export function drawWindow(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  chrome: boolean,
  o: WindowOptions = {},
): void {
  if (r.w <= 0 || r.h <= 0) return;
  const dim = clamp(o.dim ?? 0);
  const rad = Math.min(RADIUS, r.w / 2, r.h / 2);
  const body = (c: string) => mix(c, PALETTE.bg, PANE_DIM.body * dim);
  ctx.save();
  if (o.shadow !== false)
    castShadows(
      ctx,
      [SHADOW.raised, SHADOW.contact],
      () => roundedRect(ctx, r.x, r.y, r.w, r.h, rad),
      1,
      r,
    );
  roundedRect(ctx, r.x, r.y, r.w, r.h, rad);
  ctx.fillStyle = body(TERM.window);
  ctx.fill();
  ctx.save();
  ctx.clip();
  if (chrome) {
    const ch = Math.min(CHROME_H, r.h);
    ctx.fillStyle = body(TERM.chrome);
    ctx.fillRect(r.x, r.y, r.w, ch);
    ctx.fillStyle = TERM.edge;
    ctx.fillRect(r.x, r.y + ch - STROKE.hairline, r.w, STROKE.hairline);
    const d = CHROME_ART.dot;
    TERM.dots.forEach((c, i) => {
      ctx.fillStyle = c;
      ctx.beginPath();
      ctx.arc(r.x + d.x + d.gap * i, r.y + ch / 2, d.r, 0, TAU);
      ctx.fill();
    });
  }
  ctx.fillStyle = COLOR.topEdge;
  ctx.fillRect(r.x, r.y + 1, r.w, 1);
  ctx.restore();
  roundedRect(ctx, r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1, rad - 0.5);
  ctx.strokeStyle = TERM.edge;
  ctx.lineWidth = STROKE.hairline;
  ctx.stroke();
  ctx.restore();
}

// Badges: in a window's chrome bar, and anywhere a scene sets one (kit/parts.ts badge).

export interface PillOptions {
  /** Type size, px (TYPE.badge 30; micro 22). */
  size?: number;
  /** A captured string (a version a terminal printed): set in the terminal's face. */
  mono?: boolean;
  align?: "left" | "right" | "center";
  /** Scale about the pill's centre: a badge popping in from 0.9. */
  scale?: number;
}

/** A badge pill's width at `size`, px. */
export function pillWidth(
  ctx: CanvasRenderingContext2D,
  text: string,
  o: PillOptions = {},
): number {
  const size = o.size ?? TYPE.badge.size;
  const f = font(size, TYPE.badge.weight, o.mono ? MONO : undefined);
  const tw = o.mono
    ? Array.from(text).length * advance(size)
    : layout(ctx, text, f).width;
  return tw + 2 * BADGE.padEm * size;
}

/** A badge pill's height at `size`, px. */
export const pillHeight = (size: number = TYPE.badge.size): number =>
  Math.round(size * BADGE.heightEm);

/**
 * A badge (ART.md §9): a pill 1.6 em tall, night at 85 % with a paper
 * outline (2 px, 1.5 px under 26 px), its words in Space Grotesk 500 paper,
 * or mono for a captured string. Never a pillar colour. `y` is its top.
 * Returns its width.
 */
export function drawPill(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  text: string,
  o: PillOptions = {},
): number {
  const size = o.size ?? TYPE.badge.size;
  const f = font(size, TYPE.badge.weight, o.mono ? MONO : undefined);
  const h = pillHeight(size);
  const w = pillWidth(ctx, text, o);
  const align = o.align ?? "left";
  const x0 = align === "left" ? x : align === "right" ? x - w : x - w / 2;
  const sc = o.scale ?? 1;
  ctx.save();
  if (sc !== 1) {
    ctx.translate(x0 + w / 2, y + h / 2);
    ctx.scale(sc, sc);
    ctx.translate(-(x0 + w / 2), -(y + h / 2));
  }
  roundedRect(ctx, x0, y, w, h, h / 2);
  ctx.fillStyle = rgba(PALETTE.night, BADGE.fillAlpha);
  ctx.fill();
  roundedRect(ctx, x0 + 0.5, y + 0.5, w - 1, h - 1, h / 2 - 0.5);
  ctx.lineWidth = size < 26 ? STROKE.badgeMicro : STROKE.badge;
  ctx.strokeStyle = PALETTE.paper;
  ctx.stroke();
  const ty = y + h / 2 + size * 0.36;
  if (o.mono)
    drawTermLine(
      ctx,
      [run(text, PALETTE.paper)],
      x0 + BADGE.padEm * size,
      ty,
      size,
    );
  else
    drawText(ctx, text, x0 + BADGE.padEm * size, ty, {
      font: f,
      fill: PALETTE.paper,
    });
  ctx.restore();
  return w;
}

/** A badge a window's chrome bar carries: its words, and how far it has popped in. */
export interface ChromeBadge {
  text: string;
  alpha?: number;
  /** 0.9 to 1 as it pops in. */
  scale?: number;
  mono?: boolean;
}

export interface ChromeLabels {
  /** The cwd, or null for none. */
  title: string | null;
  /** The badges, right to left from the bar's right end. */
  badges?: readonly (string | ChromeBadge)[];
  /** 0 to 1: the pane dimmed; its title dims to PANE_DIM.title. */
  dim?: number;
  /**
   * A `cd`: the dot's hop, 0 to 1, and the path it left, which the title
   * cross-fades from over the hop.
   */
  hop?: { k: number; from?: string | null };
}

/** The cwd of the latest prompt on a screen (`~/work/api $ …`), or null. */
export function cwdOf(lines: readonly TermLine[]): string | null {
  for (let i = lines.length - 1; i >= 0; i--) {
    const m = /^(~\S*|\/\S*) \$(?: |$)/.exec(lineText(lines[i]));
    if (m) return m[1];
  }
  return null;
}

/**
 * The chrome bar's title and badges over a window at `r` (ART.md §6): the
 * cwd in mono 22 text3, centred, with a paper dot 14 px left of it; the
 * badges in the bar's right end, 12 px in and 4 px down, right to left.
 * When a badge would reach the centred title, the title sets left instead.
 */
export function drawChromeLabels(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: ChromeLabels,
): void {
  const C = CHROME_ART;
  const dim = clamp(o.dim ?? 0);
  ctx.save();
  roundedRect(ctx, r.x, r.y, r.w, r.h, Math.min(RADIUS, r.w / 2, r.h / 2));
  ctx.clip();
  // Badges first: they decide where the title may sit. One that has faded
  // out entirely takes no room, so a bar with it gone is the bar without it.
  let left = r.x + r.w - C.badgeInset;
  for (const b of o.badges ?? []) {
    const bd = typeof b === "string" ? { text: b } : b;
    const a = bd.alpha ?? 1;
    if (a <= 0) continue;
    const w = pillWidth(ctx, bd.text, { mono: bd.mono });
    ctx.save();
    ctx.globalAlpha *= a;
    drawPill(ctx, left, r.y + C.badgeTop, bd.text, {
      align: "right",
      mono: bd.mono,
      scale: bd.scale,
    });
    ctx.restore();
    left -= w + 10;
  }
  const f = font(C.title.size, TYPE.chrome.weight, MONO);
  const baseline = r.y + CHROME_H / 2 + 0.36 * C.title.size;
  const place = (title: string) => {
    const tw = layout(ctx, title, f).width;
    const centred = r.x + r.w / 2 - tw / 2;
    return centred + tw + 24 > left ? r.x + C.titleLeft : centred;
  };
  const titles: [string, number][] = [];
  const hop = o.hop && o.hop.k > 0 && o.hop.k < 1 ? o.hop : null;
  // The old path fades out over the hop's first half, the new one in over its second.
  if (o.title) titles.push([o.title, hop ? smoothstep(0.4, 1, hop.k) : 1]);
  if (hop?.from) titles.push([hop.from, 1 - smoothstep(0, 0.5, hop.k)]);
  ctx.globalAlpha *= lerp(1, PANE_DIM.title, dim);
  for (const [title, a] of titles) {
    if (a <= 0) continue;
    ctx.save();
    ctx.globalAlpha *= a;
    drawText(ctx, title, place(title), baseline, {
      font: f,
      fill: C.title.color,
    });
    ctx.restore();
  }
  if (o.title) {
    const x = place(o.title) - C.cwdDot.gap;
    const from = hop?.from ? place(hop.from) - C.cwdDot.gap : x;
    const k = hop ? hop.k : 1;
    const up = hop ? 4 * k * (1 - k) : 0;
    ctx.fillStyle = C.cwdDot.color;
    ctx.beginPath();
    ctx.arc(
      lerp(from, x, k),
      r.y + CHROME_H / 2 - C.cwdDot.hop * up,
      C.cwdDot.r,
      0,
      TAU,
    );
    ctx.fill();
  }
  ctx.restore();
}

export interface TermOptions {
  /** GLOBAL seconds (env.t), never local time: the spinner's frame and the cursor's blink. */
  t: number;
  /** The first line shown, fractional while a scene scrolls (default firstLine). */
  scroll?: number;
  /** 0..1 the whole pane. */
  alpha?: number;
  /** 0..1 line i (an index into `lines`): a row fading in or out. */
  lineAlpha?: (i: number) => number;
  /**
   * A block cursor, on for a beat and off for the next on global beats:
   * after the last line's text, or at a line and column. Only a prompt
   * waiting for input has one.
   */
  cursor?: boolean | { line: number; col: number };
  /** 0 to 1: the pane dimmed (PANE_DIM): its text, its title and its body. */
  dim?: number;
  /** The chrome bar's title: the cwd of the latest prompt on screen (cwdOf) unless given; null for none. */
  title?: string | null;
  /** A `cd` under way: the cwd dot's hop (drawChromeLabels). */
  hop?: ChromeLabels["hop"];
  /** Badges in the chrome bar's right end, right to left (`time-lapse` first). */
  badges?: readonly (string | ChromeBadge)[];
}

/**
 * A terminal: the window (unless the pane's is off), its chrome bar's title
 * and badges, and a screen's lines on its grid, the bottom rows of a screen
 * longer than the pane (or the top rows, for a top-anchored pane), clipped
 * to the body and faded at the right edge where 80 columns do not fit.
 * Returns where everything is.
 */
export function drawTerm(
  ctx: CanvasRenderingContext2D,
  p: Pane,
  lines: readonly TermLine[],
  o: TermOptions,
): TermLayout {
  const L = termLayout(p, lines.length, o.scroll);
  const a = o.alpha ?? 1;
  if (a <= 0) return L;
  const dim = clamp(o.dim ?? 0);
  ctx.save();
  ctx.globalAlpha *= a;
  if (p.window) drawWindow(ctx, p, p.chrome, { dim });
  if (p.chrome)
    drawChromeLabels(ctx, p, {
      title: o.title === undefined ? cwdOf(lines) : o.title,
      badges: o.badges,
      dim,
      hop: o.hop,
    });
  const { body } = L;
  if (body.w <= 0 || body.h <= 0) {
    ctx.restore();
    return L;
  }
  roundedRect(ctx, p.x, p.y, p.w, p.h, RADIUS);
  ctx.clip();
  ctx.beginPath();
  ctx.rect(body.x, body.y, body.w, body.h);
  ctx.clip();
  ctx.globalAlpha *= lerp(1, PANE_DIM.text, dim);
  const fade = p.fade >= 1 ? ([p.x + p.w - p.fade, p.x + p.w] as const) : null;
  const rows = paneRows(p);
  lines.forEach((line, i) => {
    const row = i - L.first;
    if (row <= -1 || row >= rows) return;
    const la = o.lineAlpha ? o.lineAlpha(i) : 1;
    if (la <= 0) return;
    ctx.save();
    ctx.globalAlpha *= Math.min(1, la);
    drawTermLine(ctx, line, p.x0, L.baseline(i), p.size, {
      t: o.t,
      fade,
      lineH: p.lineH,
    });
    ctx.restore();
  });
  if (o.cursor && cursorOn(o.t)) {
    const last = lines.at(-1);
    const at =
      o.cursor === true
        ? {
            line: lines.length - 1,
            col: Array.from(last === undefined ? "" : lineText(last)).length,
          }
        : o.cursor;
    const c = L.cell(at.line, at.col);
    if (fade) {
      const g = ctx.createLinearGradient(fade[0], 0, fade[1], 0);
      g.addColorStop(0, TERM.cursor);
      g.addColorStop(1, rgba(TERM.cursor, 0));
      ctx.fillStyle = g;
    } else {
      ctx.fillStyle = TERM.cursor;
    }
    ctx.fillRect(c.x, c.y, c.w, c.h);
  }
  ctx.restore();
  return L;
}

/** A terminal's window as the lit screen the vignette spares (Scene.lit). */
export const termLit = (r: Rect, alpha = 1): LitRect => ({
  x: r.x,
  y: r.y,
  w: r.w,
  h: r.h,
  alpha,
});
