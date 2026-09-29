// from jdx/hk@37937824 docs/.vitepress/theme/showreel/type.ts
// Glyph-level text layout for kinetic type. Positions come from measuring
// prefixes, so kerning survives when letters are animated one by one. Then
// the reel's copy: words that rise in one per 1/32 note and wipe away, the
// type sizes a phone viewer can read, and the must-read captions.
//
// Every face is bundled (docs/.vitepress/fonts, listed in fonts.ts) and
// loaded with FontFace before the first frame, so a local render matches
// CI's: Space Grotesk for captions and everything set in type, JetBrains
// Mono (the site's mono face) for terminals, code and hashes, and Cormorant
// Garamond (the landing page's headings) for tickets and act titles.

import { clamp, progress, swiftOut } from "./math";
import { ink } from "./kit/ink";
import { BEAT, type Section } from "./timeline";

export const DISPLAY = '"Space Grotesk", "Avenir Next", "Segoe UI", sans-serif';
/** A variable face, 100 to 800. */
export const MONO = '"JetBrains Mono", monospace';
/** Italic only (the bundled face, variable from 300 to 700): see serifItalic. */
export const SERIF = '"Cormorant Garamond", Georgia, serif';

export const font = (size: number, weight = 600, family = DISPLAY): string =>
  `${weight} ${size}px ${family}`;

/** Cormorant Garamond italic, for tickets and act titles. */
export const serifItalic = (size: number, weight = 500): string =>
  `italic ${weight} ${size}px ${SERIF}`;

// bible.ts PALETTE.paper and PALETTE.paperDim. The palette's module imports
// this one's types, so the two colours are repeated here; a test keeps them
// equal.
/** Captions and copy: warm paper. */
export const PAPER = "#f4eee3";
/**
 * Code in copy: the dimmer paper, set in mono. No pillar colour, since a
 * caption's code can name any pillar's feature.
 */
export const CODE = "#c2b6a4";

export interface Glyph {
  ch: string;
  /** Left edge relative to the start of the line. */
  x: number;
  /** Advance width. */
  w: number;
}

export interface Line {
  glyphs: Glyph[];
  width: number;
}

const cache = new Map<string, Line>();

/** Forget measurements; call once web fonts finish loading. */
export function resetTypeCache(): void {
  cache.clear();
}

/** Measure a line glyph by glyph. `tracking` is extra px after each glyph. */
export function layout(
  ctx: CanvasRenderingContext2D,
  text: string,
  fontSpec: string,
  tracking = 0,
): Line {
  const key = `${fontSpec}|${tracking}|${text}`;
  let line = cache.get(key);
  if (line) return line;
  // Animated sizes or tracking create a key per frame; keep the cache bounded.
  if (cache.size > 4000) cache.clear();
  ctx.save();
  ctx.font = fontSpec;
  const chars = Array.from(text);
  const glyphs: Glyph[] = [];
  let prefix = "";
  for (let i = 0; i < chars.length; i++) {
    const x = ctx.measureText(prefix).width + i * tracking;
    const w = ctx.measureText(chars[i]).width;
    glyphs.push({ ch: chars[i], x, w });
    prefix += chars[i];
  }
  const width =
    ctx.measureText(text).width + Math.max(0, chars.length - 1) * tracking;
  ctx.restore();
  line = { glyphs, width };
  cache.set(key, line);
  return line;
}

export interface GlyphStyle {
  dx?: number;
  dy?: number;
  /** Rotation about the glyph's baseline center, radians. */
  rot?: number;
  sx?: number;
  sy?: number;
  alpha?: number;
  fill?: string;
}

export interface TextOptions {
  font: string;
  tracking?: number;
  align?: "left" | "center" | "right";
  fill?: string;
  /** Per-glyph animation; return null to skip a glyph. */
  glyph?: (g: Glyph, i: number, n: number) => GlyphStyle | null;
}

/** Draw a line at baseline `y`, optionally animating each glyph. */
export function drawText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  opts: TextOptions,
): Line {
  ink(text, "text");
  const line = layout(ctx, text, opts.font, opts.tracking ?? 0);
  const align = opts.align ?? "left";
  const x0 =
    align === "left"
      ? x
      : align === "center"
        ? x - line.width / 2
        : x - line.width;
  ctx.save();
  ctx.font = opts.font;
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = opts.fill ?? PAPER;
  if (!opts.glyph && !opts.tracking) {
    ctx.fillText(text, x0, y);
    ctx.restore();
    return line;
  }
  const n = line.glyphs.length;
  line.glyphs.forEach((g, i) => {
    const s = opts.glyph ? opts.glyph(g, i, n) : {};
    if (!s || g.ch === " ") return;
    const a = s.alpha ?? 1;
    if (a <= 0) return;
    ctx.save();
    ctx.globalAlpha *= a;
    if (s.fill) ctx.fillStyle = s.fill;
    const cx = x0 + g.x + g.w / 2 + (s.dx ?? 0);
    ctx.translate(cx, y + (s.dy ?? 0));
    if (s.rot) ctx.rotate(s.rot);
    if (s.sx !== undefined || s.sy !== undefined)
      ctx.scale(s.sx ?? 1, s.sy ?? 1);
    ctx.fillText(g.ch, -g.w / 2, 0);
    ctx.restore();
  });
  ctx.restore();
  return line;
}

// Copy. The reel explains, so its lines are sized for the smallest screen
// that plays it: a link preview shows the frame at about 0.14x, where 88 px
// is about 12 px. Anything a muted phone viewer needs is 56 px or larger;
// 40 px is desktop detail the story never depends on. Code (in backticks) is
// mono and dim paper, a little smaller so its x-height matches.

/** How a run of words looks and moves. */
export interface WordStyle {
  font: string;
  fill: string;
  /** Words in backticks. */
  code: { font: string; fill: string };
  /** How far each word rises as it fades in, px. */
  rise: number;
}

/**
 * Space Grotesk 600 at `size` px, code in JetBrains Mono at 10/11 of it,
 * rising a little over a quarter of the size.
 */
export function wordStyle(size: number, fill = PAPER): WordStyle {
  return {
    font: font(size, 600),
    fill,
    code: { font: font(Math.round((size * 10) / 11), 600, MONO), fill: CODE },
    rise: Math.round((size * 24) / 88),
  };
}

/** The must-read line: 88 px, code at 80 px, rising 24 px. */
export const CAPTION = wordStyle(88);
/** A label a phone viewer needs: 56 px; `wordStyle(64)` for the larger ones. */
export const LABEL = wordStyle(56);

/** Words land one per 1/32 note (0.125 s at 60 BPM), each rising and fading in over its own. */
export const WORD = BEAT / 8;
/** A line leaves with a left-to-right wipe over a sixteenth. */
export const WIPE = BEAT / 4;

interface Run {
  text: string;
  code: boolean;
}
/** A word is the runs between two spaces: `target/`: is code, then a colon. */
type Word = Run[];

/** Split copy into words, with backticks marking code. */
function words(text: string): Word[] {
  const out: Word[] = [];
  let runs: Run[] = [];
  let run = "";
  let code = false;
  const endRun = () => {
    if (run) runs.push({ text: run, code });
    run = "";
  };
  for (const ch of text) {
    if (ch === "`") {
      endRun();
      code = !code;
    } else if (ch === " ") {
      endRun();
      if (runs.length) out.push(runs);
      runs = [];
    } else {
      run += ch;
    }
  }
  endRun();
  if (runs.length) out.push(runs);
  return out;
}

/** The copy as displayed, without the backticks. */
export const plain = (text: string): string => text.replaceAll("`", "");

/**
 * Words a reader has to take in: every word with a letter or digit in it,
 * so `&&` and a lone dash do not count.
 */
export function wordCount(text: string): number {
  return plain(text)
    .split(/\s+/)
    .filter((w) => /[\p{L}\p{N}]/u.test(w)).length;
}

/**
 * The reading rule (PACE rule 5): a caption stays up at least this long,
 * seconds, from the moment its first word starts to rise to the frame it
 * starts to leave: a third of a second a word and a second to write in,
 * never under 3 s. Once whole (its last word landed) it still holds at
 * least `settledTime` (storyboard.ts captionTimes).
 */
export const readingTime = (words: number): number =>
  Math.max(words / 3 + 1, 3);

/** The least time a caption holds once its last word has landed, seconds: a third of a second a word. */
export const settledTime = (words: number): number => words / 3;

/** When the first word of `text` starts to rise, if its last lands at `land`. */
export const entrance = (text: string, land: number): number =>
  land - words(text).length * WORD;

/** Each word's offset from the start of the line, and the line's width. */
function place(
  ctx: CanvasRenderingContext2D,
  list: Word[],
  style: WordStyle,
): { at: number[]; width: number } {
  // Words are spaced as prose, inside code too: a mono space reads as two.
  const space = layout(ctx, " ", style.font).width;
  let width = 0;
  const at = list.map((w, i) => {
    const x0 = width;
    for (const r of w)
      width += layout(ctx, r.text, r.code ? style.code.font : style.font).width;
    if (i < list.length - 1) width += space;
    return x0;
  });
  return { at, width };
}

/**
 * Where a left-aligned line's exit wipe has reached at `t`: nothing of it
 * shows left of this x. The edge overshoots the ink a little on both sides,
 * so the line is whole on the wipe's first frame and gone on its last.
 */
function wipeEdge(
  ctx: CanvasRenderingContext2D,
  left: number,
  width: number,
  style: WordStyle,
  t: number,
  out: number,
): number {
  const wipe = progress(out, out + WIPE, t);
  if (wipe <= 0) return -Infinity;
  const pad = 0.1 * layout(ctx, "M", style.font).width;
  return left - pad + wipe * (width + 2 * pad);
}

/**
 * Draw `text` at baseline `y` with the reel's word motion: the words rise
 * and fade in one per 1/32 note, the last landing at `land`; from `out` the
 * line wipes away left to right over a sixteenth. Times are on the caller's
 * clock (`t`). Returns the line's width.
 */
export function drawWords(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  style: WordStyle,
  t: number,
  land: number,
  out = Infinity,
  align: "left" | "center" | "right" = "left",
): number {
  const list = words(text);
  const { at, width } = place(ctx, list, style);
  const n = list.length;
  if (t < land - n * WORD || t >= out + WIPE) return width;
  ink(plain(text), "words");
  const left =
    align === "left" ? x : align === "center" ? x - width / 2 : x - width;

  ctx.save();
  const edge = wipeEdge(ctx, left, width, style, t, out);
  if (edge > -Infinity) {
    ctx.beginPath();
    ctx.rect(edge, -1e5, 2e5, 2e5);
    ctx.clip();
  }
  list.forEach((w, i) => {
    const p = progress(land - (n - i) * WORD, land - (n - i - 1) * WORD, t);
    if (p <= 0) return;
    // Opaque well before it settles, so the rise reads as a landing.
    const a = clamp(p / 0.6);
    const dy = style.rise * (1 - swiftOut(p));
    let rx = left + at[i];
    for (const r of w) {
      ctx.save();
      ctx.globalAlpha *= a;
      const spec = r.code ? style.code : style;
      rx += drawText(ctx, r.text, rx, y + dy, {
        font: spec.font,
        fill: spec.fill,
      }).width;
      ctx.restore();
    }
  });
  ctx.restore();
  return width;
}

/** One line of a caption. Backticks mark code. */
export interface CaptionLine {
  /** Section-local beat its last word lands on. */
  in: number;
  text: string;
}

/**
 * A must-read caption: one or two lines of about 36 characters at most,
 * left-aligned in the lower third. Each line lands by its own `in` beat and
 * both wipe away from `out`, in beats local to the scene's section. The
 * next caption lands at least half a beat after this one starts to leave,
 * and each line holds for its reading time (the tests check both).
 */
export interface Caption {
  /** Section-local beat the exit wipe starts on. */
  out: number;
  lines: readonly CaptionLine[];
}

/** Captions start 160 px in from the left edge. */
export const CAPTION_X = 160;
/**
 * Everything a scene leaves up while a caption is up stays above this y: the
 * captions' band runs from here to the frame's foot, and the player's
 * controls cover the last 144 px of it.
 */
export const CAPTION_TOP = 740;
/**
 * Baselines: a caption's last line sits on the lower one, clear of a
 * player's controls; a two-line caption's first line on the upper one.
 */
export const CAPTION_Y = [832, 936] as const;

/** A caption placed on the reel's clock: global seconds. */
export interface TimedCaption {
  /** The first word starts to rise. */
  start: number;
  /** The wipe starts. */
  out: number;
  /** The wipe has finished. */
  end: number;
  lines: { text: string; land: number; y: number }[];
}

/** Place a section's captions on the reel's clock. */
export function timeCaptions(
  s: Section,
  captions: readonly Caption[],
): TimedCaption[] {
  return captions.map((c) => {
    const lines = c.lines.map((l, i) => ({
      text: l.text,
      land: s.beat(l.in),
      y: CAPTION_Y[CAPTION_Y.length - c.lines.length + i],
    }));
    const out = s.beat(c.out);
    return {
      start: Math.min(...lines.map((l) => entrance(l.text, l.land))),
      out,
      end: out + WIPE,
      lines,
    };
  });
}

/** Draw whichever captions are up at global time `t`. */
export function drawCaptions(
  ctx: CanvasRenderingContext2D,
  t: number,
  captions: readonly TimedCaption[],
): void {
  // A caption can start to land while the last one wipes away. On each
  // baseline it writes in behind the wipe's edge, never over letters that
  // are still up.
  const edges = new Map<number, number>();
  for (const c of captions) {
    if (t < c.out || t >= c.end) continue;
    for (const l of c.lines) {
      const { width } = place(ctx, words(l.text), CAPTION);
      edges.set(l.y, wipeEdge(ctx, CAPTION_X, width, CAPTION, t, c.out));
    }
  }
  for (const c of captions) {
    if (t < c.start || t >= c.end) continue;
    for (const l of c.lines) {
      const edge = t < c.out ? edges.get(l.y) : undefined;
      if (edge === -Infinity) continue;
      ctx.save();
      if (edge !== undefined) {
        ctx.beginPath();
        ctx.rect(edge - 2e5, -1e5, 2e5, 2e5);
        ctx.clip();
      }
      drawWords(ctx, l.text, CAPTION_X, l.y, CAPTION, t, l.land, c.out);
      ctx.restore();
    }
  }
}
