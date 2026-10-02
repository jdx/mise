// The name card and the end card (ART.md §11, NAME_CARD): "mise-en-place"
// in Cormorant italic over the tagline, left-aligned at x 160 with the chef
// on the right. The open writes the name on and lifts the help screen's
// tagline into it; the end card stacks the name, the tagline, the install
// command typed in a strip of terminal, the platform line and mise.jdx.dev,
// each on its eighth of a beat after the end card's bar line (END_CUES),
// then holds with the chef.
//
// The copy is mise's own: the tagline is Cargo.toml's description (the
// help screen's first row, README.md's bold line), the install command and
// the Windows line are README.md's, and test/chef.test.ts holds all three
// to their sources. END_CUES and endCardState are the hooks a scene keys
// to; drawEndCard and drawOpenCard draw the whole thing from them. Every
// function is a pure function of time.

import { BEAT, PALETTE, TERM } from "../bible";
import { mix } from "../color";
import { castShadows, roundedRect } from "../fx";
import { clamp, lerp, progress } from "../math";
import {
  CODE,
  drawText,
  drawWords,
  font,
  layout,
  MONO,
  serifItalic,
  WORD,
  type WordStyle,
} from "../type";
import {
  bloomAt,
  drawChefRest,
  drawSparkle,
  GLINT,
  type Glow,
  HALO,
  sparklePoint,
} from "./chef";
import { ink } from "./ink";
import type { Rect } from "./motion";
import {
  CHEF_ART,
  COLOR,
  DUR,
  EASE,
  MOTION,
  NAME_CARD,
  RADIUS,
  SHADOW,
  TYPE,
  beats,
} from "./style";

// The copy.

/** The name, and the three syllables it writes on in, one per cue. */
export const NAME = "mise-en-place";
export const NAME_PARTS = ["mise", "-en", "-place"] as const;
/** mise's one-line description: Cargo.toml's, and the help screen's first row. */
export const TAGLINE = "Dev tools, env vars, and tasks in one CLI";
/** The install line (README.md "Install mise"): the prompt, then the command that types. */
export const INSTALL = {
  prompt: "$ ",
  command: "curl https://mise.run | sh",
} as const;
/** The platform line (footnote 7), code in backticks. */
export const PLATFORM = "macOS & Linux · Windows: `winget install jdx.mise`";
export const URL = "mise.jdx.dev";

// Type.

const NAME_FONT = serifItalic(TYPE.name.size, TYPE.name.weight);
const TAGLINE_STYLE: WordStyle = {
  font: font(TYPE.tagline.size, TYPE.tagline.weight),
  fill: PALETTE.paper,
  code: {
    font: font(Math.round((TYPE.tagline.size * 10) / 11), 500, MONO),
    fill: CODE,
  },
  rise: Math.round((TYPE.tagline.size * 24) / 88),
};
const URL_FONT = font(TYPE.url.size, TYPE.url.weight);

/** How far a line's ink starts left of its origin, px (0 where the context cannot say). */
function inkLeft(
  ctx: CanvasRenderingContext2D,
  text: string,
  f: string,
): number {
  ctx.save();
  ctx.font = f;
  const m = ctx.measureText(text);
  ctx.restore();
  return Number.isFinite(m.actualBoundingBoxLeft) ? m.actualBoundingBoxLeft : 0;
}

/** The origin that sets `text`'s ink flush with the card's left edge, x 160. */
const flush = (ctx: CanvasRenderingContext2D, text: string, f: string) =>
  NAME_CARD.x + inkLeft(ctx, text, f);

/** A word landing: `p` 0 to 1 over 1/8 beat, rising 24 px (arrive) and opaque by 60 %. */
function landing(p: number): { alpha: number; dy: number } {
  return { alpha: clamp(p / 0.6), dy: MOTION.riseIn * (1 - EASE.arrive(p)) };
}

/**
 * "mise-en-place" at baseline `y`, flush left: each syllable rising and
 * fading in over 1/8 beat from its own time `at[i]` (seconds on `t`'s
 * clock), placed where the whole word sets it, so the kerning between
 * syllables is the word's.
 */
export function drawName(
  ctx: CanvasRenderingContext2D,
  t: number,
  at: readonly [number, number, number],
  o: { y?: number; alpha?: number } = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0 || t < at[0]) return;
  const y = o.y ?? NAME_CARD.nameY;
  const x0 = flush(ctx, NAME, NAME_FONT);
  const line = layout(ctx, NAME, NAME_FONT);
  let first = 0;
  NAME_PARTS.forEach((part, i) => {
    const p = progress(at[i], at[i] + beats(DUR.flick), t);
    const start = first;
    first += part.length;
    if (p <= 0) return;
    const l = landing(p);
    ctx.save();
    ctx.globalAlpha *= a * l.alpha;
    drawText(ctx, part, x0 + line.glyphs[start].x, y + l.dy, {
      font: NAME_FONT,
      fill: PALETTE.paper,
    });
    ctx.restore();
  });
}

/** The tagline's first word starts to rise at `from`; the rest follow one per 1/32 note (drawWords). */
export function drawTagline(
  ctx: CanvasRenderingContext2D,
  t: number,
  from: number,
  o: { alpha?: number } = {},
): void {
  const a = o.alpha ?? 1;
  if (a <= 0) return;
  const n = TAGLINE.split(" ").length;
  ctx.save();
  ctx.globalAlpha *= a;
  drawWords(
    ctx,
    TAGLINE,
    flush(ctx, TAGLINE, TAGLINE_STYLE.font),
    NAME_CARD.taglineY,
    TAGLINE_STYLE,
    t,
    from + n * WORD,
  );
  ctx.restore();
}

/** The install strip's rect, and its text's first column and baseline. */
export function installBox(): Rect & { x0: number; baseline: number } {
  const I = NAME_CARD.install;
  const size = TYPE.install.size;
  const cols = (INSTALL.prompt + INSTALL.command).length;
  return {
    x: NAME_CARD.x,
    y: I.y,
    w: cols * 0.6 * size + 2 * I.pad,
    h: I.h,
    x0: NAME_CARD.x + I.pad,
    baseline: Math.round(I.y + I.h / 2 + 0.36 * size),
  };
}

/**
 * The install line: a strip of terminal (TERM.window, TERM.edge, SHADOW.small
 * and contact) rising in over `boxIn` (0 to 1), the prompt in the neutral
 * prompt colour and the first `typed` characters of the command on the
 * terminal's grid (JetBrains Mono, TYPE.install), with the block cursor after
 * them while `cursor` is set.
 */
export function drawInstall(
  ctx: CanvasRenderingContext2D,
  boxIn: number,
  typed: number,
  cursor: boolean,
): void {
  if (boxIn <= 0) return;
  const r = installBox();
  const size = TYPE.install.size;
  const adv = 0.6 * size;
  const e = EASE.arrive(clamp(boxIn));
  ctx.save();
  ctx.globalAlpha *= e;
  ctx.translate(0, MOTION.riseIn * (1 - e));
  castShadows(ctx, [SHADOW.small, SHADOW.contact], () =>
    roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.lane),
  );
  roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.lane);
  ctx.fillStyle = TERM.window;
  ctx.fill();
  roundedRect(ctx, r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1, RADIUS.lane - 0.5);
  ctx.strokeStyle = TERM.edge;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.fillStyle = COLOR.topEdge;
  ctx.fillRect(r.x + RADIUS.lane, r.y + 1, r.w - 2 * RADIUS.lane, 1);
  const n = Math.max(0, Math.min(INSTALL.command.length, Math.floor(typed)));
  const text = INSTALL.prompt + INSTALL.command.slice(0, n);
  ink(text, "text");
  ctx.font = font(size, TYPE.install.weight, MONO);
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  // One character per column, as the terminal sets them (kit/term.ts).
  Array.from(text).forEach((ch, i) => {
    if (ch === " ") return;
    ctx.fillStyle = i < INSTALL.prompt.length ? COLOR.prompt : TERM.text;
    ctx.fillText(ch, r.x0 + i * adv, r.baseline);
  });
  if (cursor) {
    ctx.fillStyle = TERM.cursor;
    ctx.fillRect(
      r.x0 + text.length * adv,
      r.baseline - 0.925 * size,
      adv,
      1.25 * size,
    );
  }
  ctx.restore();
}

/** Copy with `code` in backticks at baseline `y`, flush left: prose in Space Grotesk, code in mono. */
function codeLine(
  ctx: CanvasRenderingContext2D,
  text: string,
  y: number,
  size: number,
  codeSize: number,
  fill: string,
  codeFill: string,
): void {
  const parts = text.split("`");
  const prose = font(size, TYPE.platform.weight);
  const code = font(codeSize, TYPE.platform.weight, MONO);
  let x = flush(ctx, parts[0] || parts[1] || "", parts[0] ? prose : code);
  parts.forEach((part, i) => {
    if (!part) return;
    x += drawText(ctx, part, x, y, {
      font: i % 2 ? code : prose,
      fill: i % 2 ? codeFill : fill,
    }).width;
  });
}

/** The platform line (footnote 7): Space Grotesk 40 paperDim, code mono 37 text2, at `alpha`. */
export function drawPlatform(
  ctx: CanvasRenderingContext2D,
  alpha: number,
): void {
  if (alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= alpha;
  codeLine(
    ctx,
    PLATFORM,
    NAME_CARD.platformY,
    TYPE.platform.size,
    TYPE.footnote.code,
    PALETTE.paperDim,
    PALETTE.text2,
  );
  ctx.restore();
}

/** mise.jdx.dev, landing: `p` 0 to 1 over 1/8 beat (rise 24 px and fade). */
export function drawUrl(ctx: CanvasRenderingContext2D, p: number): void {
  if (p <= 0) return;
  const l = landing(p);
  ctx.save();
  ctx.globalAlpha *= l.alpha;
  drawText(ctx, URL, flush(ctx, URL, URL_FONT), NAME_CARD.urlY + l.dy, {
    font: URL_FONT,
    fill: PALETTE.paper,
  });
  ctx.restore();
}

// The end card, on the grid.

/** `k` eighths of a beat, seconds. */
const eighths = (k: number): number => (k * BEAT) / 8;

/**
 * The end card's cues, seconds from its bar line (E0, `sec("end").start`),
 * each on an eighth of a beat. They keep the motion jdx's sung line gave
 * the card when it was keyed to the recording's measured onsets (plan v3
 * §5, board5.py), each moved to its nearest eighth: the name a syllable at
 * 0, 0.375 and 0.625 s, the tagline from 1.5, the install strip landing at
 * 2.75 and its command typed by 5.25, the platform line from 6.25,
 * mise.jdx.dev landing at 7.5 and the glint from 8.25. The score's end
 * card (score/end.ts) sounds on the same cues.
 */
export const END_CUES = {
  /** "mise" writes on, on the bar line, with the chef's bloom. */
  mise: 0,
  en: eighths(3),
  place: eighths(5),
  /** The tagline's first word starts to rise. */
  tagline: eighths(12),
  /** The install strip has risen in, and the command's first character types. */
  install: eighths(22),
  /** The command's last character types, and the cursor goes. */
  typed: eighths(42),
  /** The platform line starts to fade in. */
  platform: eighths(50),
  /** mise.jdx.dev has landed. */
  url: eighths(60),
  /** The glint starts across the hat. */
  glint: eighths(66),
} as const;

export type EndCues = { readonly [K in keyof typeof END_CUES]: number };

/** Everything on the end card at a moment, as numbers: the hooks a scene keys to. */
export interface EndCardState {
  /** Each syllable's landing, 0 to 1 over 1/8 beat from its cue. */
  name: [number, number, number];
  /** The tagline's first word starts to rise here (seconds). */
  taglineFrom: number;
  /** The install strip rising in, 0 to 1 over 1/4 beat, landing on its cue. */
  box: number;
  /** Characters of the command typed: one as the strip lands, the last on `typed`. */
  typed: number;
  /** The cursor shows while the command types. */
  cursor: boolean;
  /** The platform line, fading in over 1/2 beat. */
  platform: number;
  /** mise.jdx.dev landing on its cue, 0 to 1 over the 1/8 beat before it. */
  url: number;
  /** The glint's sweep, 0 to 1 over 1/2 beat (arrive). */
  glint: number;
  /** The sparkle on the right lobe, peaking as the glint ends. */
  sparkle: number;
  /** The chef's backlight: the bloom swells on "mise" and settles to the halo. */
  glow: Glow;
}

/**
 * The end card at local second `lt` (from E0): every moment starts on its
 * cue, so the frame on the end's bar line (lt 0) is the chef at rest and
 * nothing else, and every move is over by 9 s, after which only the motes
 * move.
 */
export function endCardState(lt: number, c: EndCues = END_CUES): EndCardState {
  const word = beats(DUR.flick);
  const n = INSTALL.command.length;
  const typing = progress(c.install, c.typed, lt);
  const glintEnd = c.glint + GLINT.dur;
  return {
    name: [c.mise, c.en, c.place].map((at) => progress(at, at + word, lt)) as [
      number,
      number,
      number,
    ],
    taglineFrom: c.tagline,
    box: progress(c.install - beats(DUR.tick), c.install, lt),
    typed:
      lt < c.install ? 0 : Math.min(n, 1 + Math.floor((n - 1) * typing + 1e-9)),
    cursor: lt >= c.install - beats(DUR.tick) && typing < 1,
    platform: EASE.glide(
      progress(c.platform, c.platform + beats(DUR.half), lt),
    ),
    // It lands on its cue (ART.md §11 End: arriving on it), as the install
    // strip lands on its own: in over the 1/8 beat before it.
    url: progress(c.url - word, c.url, lt),
    glint: EASE.arrive(progress(c.glint, glintEnd, lt)),
    sparkle: progress(glintEnd - GLINT.sparkle, glintEnd + GLINT.sparkle, lt),
    glow: bloomAt((lt - c.mise) / BEAT, HALO),
  };
}

/** The rects the end card's lines cover, for the motes to keep clear of. */
export function endCardRects(ctx: CanvasRenderingContext2D): Rect[] {
  const w = (text: string, f: string) => layout(ctx, text, f).width;
  const N = NAME_CARD;
  const line = (y: number, size: number, width: number): Rect => ({
    x: N.x,
    y: y - size,
    w: width,
    h: size * 1.25,
  });
  return [
    line(N.nameY, TYPE.name.size, w(NAME, NAME_FONT)),
    line(N.taglineY, TYPE.tagline.size, w(TAGLINE, TAGLINE_STYLE.font)),
    installBox(),
    line(
      N.platformY,
      TYPE.platform.size,
      w(
        PLATFORM.replaceAll("`", ""),
        font(TYPE.platform.size, TYPE.platform.weight),
      ),
    ),
    line(N.urlY, TYPE.url.size, w(URL, URL_FONT)),
  ];
}

/**
 * The end card at local second `lt` (GLOBAL `t` for the motes): the chef
 * at rest with its bloom, glint and sparkle, and the name card's stack.
 */
export function drawEndCard(
  ctx: CanvasRenderingContext2D,
  lt: number,
  t: number,
  o: { cues?: EndCues; place?: Rect } = {},
): void {
  const c = o.cues ?? END_CUES;
  const s = endCardState(lt, c);
  const place = o.place ?? CHEF_ART.place;
  drawChefRest(ctx, t, place, {
    glow: s.glow,
    glint: s.glint,
    avoid: endCardRects(ctx),
  });
  const sp = sparklePoint(place);
  drawSparkle(ctx, sp.x, sp.y, s.sparkle);
  drawName(ctx, lt, [c.mise, c.en, c.place]);
  drawTagline(ctx, lt, s.taglineFrom);
  drawInstall(ctx, s.box, s.typed, s.cursor);
  drawPlatform(ctx, s.platform);
  drawUrl(ctx, s.url);
}

// The open's name card (ART.md §11 Open), in the open's beats.

/** The open's name-card cues, section beats. */
export const OPEN_CARD_CUE = {
  /** The tagline lifts out of the help screen into the card. */
  lift: 4,
  liftDur: DUR.move,
  /** "mise", "-en", "-place", 1/4 beat apart. */
  name: 4.25,
  nameGap: DUR.tick,
  /** The card leaves. */
  leave: 6.5,
} as const;

/** How far the open's card rises as it leaves, px (ART.md §11). */
const LEAVE_RISE = 20;

export interface OpenCardOptions {
  /**
   * The tagline as the help screen printed it: its captured text, its
   * first column's left edge and baseline, its mono size and colour. Null
   * (no capture) writes the tagline on in place instead.
   */
  from: {
    text: string;
    x: number;
    y: number;
    size: number;
    color?: string;
  } | null;
}

/**
 * The open's name card at beat `b`: from beat 4 a copy of the help
 * screen's tagline row lifts into the card's tagline (move, 3/4 beat),
 * crossing from mono to Space Grotesk 52 between 40 % and 60 % of the way,
 * the two kept the same width so the line reads as one; the name writes
 * on from 4¼; from 6½ the card rises 20 px and fades (leave, 3/8 beat).
 */
export function drawOpenCard(
  ctx: CanvasRenderingContext2D,
  b: number,
  o: OpenCardOptions,
): void {
  const C = OPEN_CARD_CUE;
  if (b < C.lift) return;
  const leave = EASE.leave(progress(C.leave, C.leave + DUR.exit, b));
  if (leave >= 1) return;
  ctx.save();
  ctx.globalAlpha *= 1 - leave;
  ctx.translate(0, -LEAVE_RISE * leave);
  const at = (i: number) => (C.name + i * C.nameGap) * BEAT;
  drawName(ctx, b * BEAT, [at(0), at(1), at(2)]);
  const u = progress(C.lift, C.lift + C.liftDur, b);
  if (!o.from) {
    drawTagline(ctx, b * BEAT, C.lift * BEAT);
  } else {
    const f = o.from;
    const k = EASE.move(u);
    const cross = progress(0.4, 0.6, u);
    const target = {
      x: flush(ctx, TAGLINE, TAGLINE_STYLE.font),
      y: NAME_CARD.taglineY,
    };
    const monoW = Array.from(f.text).length * 0.6 * f.size;
    const sansW = layout(ctx, TAGLINE, TAGLINE_STYLE.font).width;
    const x = lerp(f.x, target.x, k);
    const y = lerp(f.y, target.y, k);
    if (cross < 1) {
      // The copy in mono on the terminal's grid, growing to the card's width.
      const size = lerp(f.size, sansW / (0.6 * Array.from(f.text).length), k);
      ctx.save();
      ctx.globalAlpha *= 1 - cross;
      ink(f.text, "text");
      ctx.font = font(size, 400, MONO);
      ctx.fillStyle = mix(f.color ?? TERM.text, PALETTE.paper, k);
      Array.from(f.text).forEach((ch, i) => {
        if (ch !== " ") ctx.fillText(ch, x + i * 0.6 * size, y);
      });
      ctx.restore();
    }
    if (cross > 0) {
      // The card's tagline, shrunk from the mono line's width to its own.
      const size = lerp(
        (TYPE.tagline.size * monoW) / sansW,
        TYPE.tagline.size,
        k,
      );
      ctx.save();
      ctx.globalAlpha *= cross;
      drawText(ctx, TAGLINE, x, y, {
        font: font(size, TYPE.tagline.weight),
        fill: PALETTE.paper,
      });
      ctx.restore();
    }
  }
  ctx.restore();
}

/** The open's card lines, for the motes to keep clear of. */
export function openCardRects(ctx: CanvasRenderingContext2D): Rect[] {
  return endCardRects(ctx).slice(0, 2);
}
