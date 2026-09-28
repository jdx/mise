// The open's name card, as the open scene choreographs it: kit/namecard.ts
// drawOpenCard's tagline lift (a copy of the help screen's tagline row
// moving into the card and crossing from mono to Space Grotesk), with the
// name written on a quarter beat later than the kit's OPEN_CARD_CUE.name.
//
// Why a quarter later: the tagline's flight from its row to the card's
// tagline (452) passes through the name's band (its x-height sits on 290
// to 360) for the first part of its move, so at the kit's 4¼ the first
// syllable rose through the moving line. From 4½ the line is most of the
// way home, below the name's baseline, and the three syllables keep the
// kit's quarter-beat rhyme (4½, 4¾, 5), landing clear of it.
//
// The crossing is glyph on glyph: over a short window at the move's peak
// speed each character of the mono copy and the same character of the
// card's line are set at one x, sliding together from the mono grid's
// place to the proportional face's, so the two faces never stand beside
// each other as a ghosted second line (the director review's M1: at 40 to
// 60 % of the move, the whole mono line and the whole sans line, each at
// half alpha, sat a few px apart for five frames).
//
// The copy, fonts and flush-left placement are namecard.ts's own, so the
// card it lands on is the end card's (endCardRects, openCardRects).

import { BEAT, PALETTE, TERM } from "../../bible";
import { mix } from "../../color";
import { lerp, progress, smoothstep } from "../../math";
import { drawText, font, layout, MONO } from "../../type";
import { ink } from "../../kit/ink";
import { type Presence, span } from "../../kit/motion";
import { drawName, OPEN_CARD_CUE, TAGLINE } from "../../kit/namecard";
import { DUR, EASE, NAME_CARD, TYPE } from "../../kit/style";

/** The name's syllables, section beats: a quarter after the kit's, a quarter apart. */
const nameAt = (i: number): number =>
  OPEN_CARD_CUE.name + DUR.tick + i * OPEN_CARD_CUE.nameGap;
export const NAME_AT = [nameAt(0), nameAt(1), nameAt(2)] as const;

/**
 * How far the card (and the chef after it) rises as it leaves, px: ART.md
 * §11 Open's −20, lifted to 32 so the move carries the exit and the fade
 * can be short.
 */
export const LEAVE_RISE = 32;
/**
 * The card leaves from OPEN_CARD_CUE.leave over 3/8 beat (DUR.exit), as
 * every exit does (kit/motion.ts exit): rising on leave, fading on glide.
 * Over the half beat it had, a paper chef at 30 to 60 % read as a grey
 * chef for a third of a second (the director review's m1); the chef
 * follows 1/8 beat later on the same curve.
 */
export const LEAVE_DUR = DUR.exit;
export const leaveOf = (
  b: number,
  at: number = OPEN_CARD_CUE.leave,
): Presence => ({
  alpha: 1 - span(b, at, LEAVE_DUR, "glide"),
  dy: -LEAVE_RISE * span(b, at, LEAVE_DUR, "leave"),
});

/**
 * The crossing from mono to Space Grotesk, as fractions of the tagline's
 * move: a short window round its middle, where EASE.move is fastest, so
 * the motion hides the swap.
 */
const CROSS = [0.46, 0.54] as const;

const TAGLINE_FONT = font(TYPE.tagline.size, TYPE.tagline.weight);

/** The origin that sets `text`'s ink flush with the card's left edge (namecard.ts flush). */
function flush(ctx: CanvasRenderingContext2D, text: string, f: string): number {
  ctx.save();
  ctx.font = f;
  const m = ctx.measureText(text);
  ctx.restore();
  return (
    NAME_CARD.x +
    (Number.isFinite(m.actualBoundingBoxLeft) ? m.actualBoundingBoxLeft : 0)
  );
}

/**
 * When the open's name card moves, section beats: the tagline's lift, the
 * name's three syllables, the card's leave. Default the kit's (with the
 * name a quarter later, NAME_AT); the open passes its own schedule's.
 */
export interface OpenCardCue {
  lift: number;
  name: readonly [number, number, number];
  leave: number;
}

const CARD_CUE: OpenCardCue = {
  lift: OPEN_CARD_CUE.lift,
  name: NAME_AT,
  leave: OPEN_CARD_CUE.leave,
};

export interface TaglineFrom {
  text: string;
  x: number;
  y: number;
  size: number;
  color?: string;
}

/**
 * The open's name card at beat `b`: from OPEN_CARD_CUE.lift a copy of the
 * help screen's tagline row lifts into the card's tagline (move, 3/4
 * beat), crossing from mono to Space Grotesk glyph on glyph over CROSS,
 * the two kept the same width; the name writes on at NAME_AT; from
 * OPEN_CARD_CUE.leave the card rises and fades (leaveOf). With no capture
 * (`from` null) the tagline writes on in place.
 */
export function drawOpenNameCard(
  ctx: CanvasRenderingContext2D,
  b: number,
  from: TaglineFrom | null,
  cue: OpenCardCue = CARD_CUE,
): void {
  const C = { ...cue, liftDur: OPEN_CARD_CUE.liftDur };
  if (b < Math.min(C.lift, C.name[0])) return;
  const leave = leaveOf(b, C.leave);
  if (leave.alpha <= 0) return;
  ctx.save();
  ctx.globalAlpha *= leave.alpha;
  ctx.translate(0, leave.dy);
  drawName(ctx, b * BEAT, [
    C.name[0] * BEAT,
    C.name[1] * BEAT,
    C.name[2] * BEAT,
  ]);
  if (b < C.lift) {
    ctx.restore();
    return;
  }
  const u = progress(C.lift, C.lift + C.liftDur, b);
  const target = {
    x: flush(ctx, TAGLINE, TAGLINE_FONT),
    y: NAME_CARD.taglineY,
  };
  if (!from) {
    ctx.save();
    ctx.globalAlpha *= EASE.arrive(u);
    drawText(ctx, TAGLINE, target.x, target.y, {
      font: TAGLINE_FONT,
      fill: PALETTE.paper,
    });
    ctx.restore();
    ctx.restore();
    return;
  }
  const k = EASE.move(u);
  const cross = progress(CROSS[0], CROSS[1], u);
  // Where the glyphs stand between the two faces' places: 0 on the mono
  // grid, 1 on the proportional face's own advances.
  const slide = smoothstep(0, 1, cross);
  const chars = Array.from(from.text);
  const monoW = chars.length * 0.6 * from.size;
  const sansW = layout(ctx, TAGLINE, TAGLINE_FONT).width;
  const x = lerp(from.x, target.x, k);
  const y = lerp(from.y, target.y, k);
  // Both faces at one width: the mono copy grows to the card's line, the
  // card's line starts at the mono copy's.
  const monoSize = lerp(from.size, sansW / (0.6 * chars.length), k);
  const sansSize = lerp(
    (TYPE.tagline.size * monoW) / sansW,
    TYPE.tagline.size,
    k,
  );
  const sansFont = font(sansSize, TYPE.tagline.weight);
  const sans = layout(ctx, TAGLINE, sansFont).glyphs;
  // The same text in both faces (the capture's row is the tagline), so
  // glyph i of one is glyph i of the other; where they differ, each face
  // keeps its own places. The pair share a centre, not a left edge: a
  // narrow sans glyph (l, i, t) set at a mono cell's left edge left the
  // two faces' strokes side by side.
  const same = from.text === TAGLINE && sans.length === chars.length;
  const half = 0.3 * monoSize;
  const monoX = (i: number) => i * 0.6 * monoSize;
  const centre = (i: number) =>
    same
      ? lerp(monoX(i) + half, sans[i].x + sans[i].w / 2, slide)
      : monoX(i) + half;
  if (cross < 1) {
    ctx.save();
    ctx.globalAlpha *= 1 - cross;
    ink(from.text, "text");
    ctx.font = font(monoSize, 400, MONO);
    ctx.textAlign = "left";
    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = mix(from.color ?? TERM.text, PALETTE.paper, k);
    chars.forEach((ch, i) => {
      if (ch !== " ") ctx.fillText(ch, x + centre(i) - half, y);
    });
    ctx.restore();
  }
  if (cross > 0) {
    ctx.save();
    ctx.globalAlpha *= cross;
    drawText(ctx, TAGLINE, x, y, {
      font: sansFont,
      fill: PALETTE.paper,
      glyph: same ? (g, i) => ({ dx: centre(i) - g.w / 2 - g.x }) : undefined,
    });
    ctx.restore();
  }
  ctx.restore();
}
