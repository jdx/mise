// The morph's card and its lobes, as the morph takes the card apart and
// forms the chef big in the middle of the stage.
//
// The card: where a folded header's
// annotation pill sits and what it says, so the pills can fade over the
// bands as they lift (kit/parts.ts card draws them, but does not return
// them). The notes follow card()'s own folding (kit/parts.ts cardRows) for
// a card with every table folded, as the climax's card is (kit/rest.ts
// CLONE_CARD): a table's header and "n lines", and consecutive `[tasks.*]`
// tables folded to the first one's header and "n more tables".

import { type Pillar, PILLAR } from "../../bible";
import { mixRGB, type RGB, rgb } from "../../color";
import { glow } from "../../fx";
import {
  drawLobeDisc,
  lobeAt,
  lobePillar,
  MORPH_CUE,
  morphState,
} from "../../kit/chef";
import {
  type Curve,
  curveAt,
  lerpRect,
  type Pt,
  type Rect,
  span,
} from "../../kit/motion";
import { type CardRows, tomlBlocks } from "../../kit/parts";
import {
  CARD_ART,
  CHEF_ART,
  COLOR,
  DUR,
  EASE,
  GLOW,
  MOTION,
  PILLAR_BAND,
  PILLAR_BRIGHT,
  TYPE,
} from "../../kit/style";
import { clamp, lerp, progress, smoothstep, TAU } from "../../math";
import { drawText, font, MONO } from "../../type";

const plural = (n: number, one: string): string =>
  `${n} ${one}${n === 1 ? "" : "s"}`;

/** Each folded header's annotation, by header, for a card with every table folded. */
export function foldedNotes(text: string): Map<string, string> {
  const blocks = tomlBlocks(text);
  const notes = new Map<string, string>();
  for (let i = 0; i < blocks.length; i++) {
    const blk = blocks[i];
    const task = /^\[tasks\./.test(blk.header);
    let j = i;
    while (
      task &&
      j + 1 < blocks.length &&
      /^\[tasks\./.test(blocks[j + 1].header)
    )
      j++;
    const n = blk.body.length;
    const note =
      j > i ? plural(j - i, "more table") : n ? plural(n, "line") : undefined;
    if (note) notes.set(blk.header, note);
    i = j;
  }
  return notes;
}

/** Where card() sets a folded header's annotation pill: its left edge and its middle. */
export function pillAt(row: CardRows[number]): { x: number; midY: number } {
  const size = TYPE.card.size;
  return {
    x: row.x + (row.w ?? 0) + CARD_ART.annotation.gap,
    midY: row.y - 0.8 * size + size / 2,
  };
}

// Where the chef forms (the director review's M3): the climax's card sits
// in the top-right corner, and the brand beat used to play there too, in
// the end card's 500 × 526 place with 60 % of the frame empty for 6.4 s.
// The chef now forms 1.3× and centred (MORPH_BIG), takes its toque there,
// and glides to CHEF_ART.place over beats 5 to 6.5 while its tint drains
// (glide: a beat-and-a-half move on the vocabulary's swift rest-to-rest
// curve), so it is at rest in its end-card place from beat 6.5, a beat and
// a half before the morph|end frame.

/** How much bigger the chef forms than its end-card place. */
const BIG = 1.3;
/** The chef's place while it forms: centred across the frame, a little below the middle of the actor band. */
export const MORPH_BIG: Rect = {
  x: 960 - (CHEF_ART.place.w * BIG) / 2,
  y: 470 - (CHEF_ART.place.h * BIG) / 2,
  w: CHEF_ART.place.w * BIG,
  h: CHEF_ART.place.h * BIG,
};
/** The glide to the end-card place, beats: the lamp's too (fx.ts lampAt). */
export const TO_PLACE = CHEF_ART.glide;

/** Where the chef stands at beat `b`: MORPH_BIG, gliding to CHEF_ART.place over TO_PLACE; exactly CHEF_ART.place from beat 6.5. */
export function morphPlace(b: number): Rect {
  const k = span(b, TO_PLACE[0], TO_PLACE[1] - TO_PLACE[0], "glide");
  return k >= 1 ? CHEF_ART.place : lerpRect(MORPH_BIG, CHEF_ART.place, k);
}

// The lobes in flight. kit/chef.ts drawMorphChef draws them itself when it
// is given the bands; the morph gives it none and draws them here, with
// the kit's disc (drawLobeDisc) and its hover height, on its own flights
// (the director review's M2, M6 and m8):
//
// - Each band flies on its own route, so none crosses another as they
//   leave the card: tools, the top band, arcs up and over to the lowest
//   lobe; env flies nearly straight; tasks, the bottom band, runs low and
//   rises last into the right lobe. Each is drawn in front of the one it
//   follows (tools, env, tasks), so a band moving up is in front.
// - Each shrinks to the lobe's width on its own clock from its lift-off
//   (travel, 3/4 beat) while its height grows only with its flight, so the
//   three are three short pills a band's height apart before any of them
//   grows toward a disc (at the kit's flight-bound width they stayed wide
//   bars and piled into one another over the card for 14 frames).
// - Each lands on its own beat (MORPH_CUE.lands: 2, 2.5, 3) on glide,
//   which ends decisively (the kit's travel curve crept in for its last
//   third, so all three read as landed by 1.5 and the hover as frozen for
//   two seconds), and ignites as it lands: GLOW.ignite's pulse in its
//   pillar behind it, settling to a steady glow while it hovers.
// - Each keeps its header's text, sliding from where the card set it to
//   the disc's middle and darkening to ink as the band rounds, until the
//   toque drops: three discs over a bald head read as the card's three
//   headers, not as three unlabelled circles.
// - Over the hover each floats ±8 px on a two-beat sine, a third of a turn
//   from its neighbours, from its landing; all three wind up
//   MOTION.anticipate px over the 1/8 beat before the drop.

/** How far above its lobe a disc hovers before the toque drops, px (kit/chef.ts HOVER: the hat's own hover). */
const HOVER = 60;
/**
 * The hover float: amplitude px, period beats, easing in over this long
 * from each landing and out into the wind-up. It eases in over a quarter
 * and out from 3⅝ so the last lobe, landing on 3, floats too: with half a
 * beat in and out from 3½, the discs stood nearly still for the half
 * second before the wind-up.
 */
const FLOAT = 8;
const FLOAT_PERIOD = 2;
const FLOAT_IN = DUR.tick;
const FLOAT_OUT = [3.625, MORPH_CUE.drop - DUR.anticipate] as const;
/** The wind-up before the drop. */
const WIND = [MORPH_CUE.drop - DUR.anticipate, MORPH_CUE.drop] as const;
/** Each band's lift-off, beats: the kit's dip, then a staggered start. */
const liftOff = (i: number): number => MORPH_CUE.fly + i * MORPH_CUE.stagger;
/** Each band's shrink to its lobe's width, from its lift-off. */
const SHRINK = DUR.move;
/** Each route's bulge, as a fraction of its length: up and over, straight, low (ART.md §11 Morph). */
const BULGE = [0.28, 0.04, -0.18] as const;
/** The labels leave over the drop's first quarter beat. */
const LABEL_OUT = [MORPH_CUE.drop, MORPH_CUE.drop + DUR.tick] as const;
/**
 * A lobe is its table's pillar, so a header that names a sub-table (the
 * folded card's first tasks header, `[tasks.lint]`) folds its dotted part
 * away as its band lifts: the closing bracket slides left over it while it
 * fades, over this much of the band's shrink (lobeShape), and the lobe
 * reads `[tasks]` from there on. The card at the bar line keeps the file's
 * own header (kit/rest.ts clone|morph); a header with no dotted part
 * (`[tools]`, `[env]`) is drawn whole.
 */
const LABEL_FOLD = 0.6;

/** A header split round its dotted part: `[tasks` `.lint` `]`; `sub` is empty for `[tools]`. */
function labelParts(text: string): {
  head: string;
  sub: string;
  close: string;
} {
  const m = /^(\[[^.\]]+)(\.[^\]]*)(\].*)$/.exec(text);
  return m
    ? { head: m[1], sub: m[2], close: m[3] }
    : { head: text, sub: "", close: "" };
}

/** How dark a disc's label is: COLOR.ink at this alpha over the pillar. */
const LABEL_INK = 0.8;
/** The discs' glow: GLOW.ignite's strength, this far past the disc's rim. */
const GLOW_REACH = GLOW.ignite.radius;
const GLOW_STEADY = 0.5;

/** Relative luminance of a colour (sRGB weights on the stored values: only compared). */
const lum = (c: string | RGB): number => {
  const [r, g, b] = typeof c === "string" ? rgb(c) : c;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

/**
 * How lit a disc is (drawLobeDisc's `k`) when its label turns from the
 * pillar to ink: the first k at which the disc's fill under the label
 * (PILLAR_BAND to half way between the rim's PILLAR and the centre's
 * PILLAR_BRIGHT) is nearer the pillar's luminance than the ink's, so the
 * label has the stronger contrast on every frame.
 */
const inkSwitch = (() => {
  const memo = new Map<Pillar, number>();
  return (p: Pillar): number => {
    let k = memo.get(p);
    if (k === undefined) {
      const mid = (lum(PILLAR[p]) + lum(COLOR.ink)) / 2;
      // Half way between the disc's rim and its centre, where the label sits.
      const lit = mixRGB(PILLAR[p], PILLAR_BRIGHT[p], 0.5);
      let lo = 0;
      let hi = 1;
      for (let n = 0; n < 24; n++) {
        const m = (lo + hi) / 2;
        if (lum(mixRGB(PILLAR_BAND[p], lit, m)) < mid) lo = m;
        else hi = m;
      }
      k = hi;
      memo.set(p, k);
    }
    return k;
  };
})();

/** Band `i`'s flight at beat `b`, 0 on the card to 1 over its lobe: glide from its lift-off to its beat. */
export function lobeFlight(i: number, b: number): number {
  return span(b, liftOff(i), MORPH_CUE.lands[i] - liftOff(i), "glide");
}

/**
 * Band `i`'s shape at beat `b`: 0 a band, 1 as narrow as its lobe (travel
 * over SHRINK from its lift-off: arrive's steep start took a sixth of the
 * width in the first frame, a snap at the lift).
 */
const lobeShape = (i: number, b: number): number =>
  span(b, liftOff(i), SHRINK, "travel");

/** Lobe `i`'s extra vertical offset at beat `b`, px: the hover float and the wind-up. */
export function lobeLift(i: number, b: number): number {
  const land = MORPH_CUE.lands[i];
  const env =
    smoothstep(land, land + FLOAT_IN, b) *
    (1 - smoothstep(FLOAT_OUT[0], FLOAT_OUT[1], b));
  const float =
    env > 0
      ? FLOAT *
        env *
        Math.sin((TAU * (b - land)) / FLOAT_PERIOD + (i * TAU) / 3)
      : 0;
  const w = progress(WIND[0], WIND[1], b);
  const wind = w > 0 && w < 1 ? -MOTION.anticipate * Math.sin(Math.PI * w) : 0;
  return float + wind;
}

/** A route from `a` to `z` bulging `lift` of its length to the left of travel (motion.arc's, signed: below the line when negative). */
function route(a: Pt, z: Pt, lift: number): Curve {
  const dx = z.x - a.x;
  const dy = z.y - a.y;
  const flip = dx < 0 ? -1 : 1;
  return {
    a,
    b: z,
    c: {
      x: (a.x + z.x) / 2 + dy * lift * flip,
      y: (a.y + z.y) / 2 - Math.abs(dx) * lift,
    },
  };
}

/** How brightly lobe `i` glows at beat `b`, 0 to 1: building on its approach, a pulse on its landing, then steady. */
function lobeGlow(i: number, b: number): number {
  const land = MORPH_CUE.lands[i];
  const near = smoothstep(0.7, 1, lobeFlight(i, b));
  const pulse = Math.sin(Math.PI * clamp(progress(land, land + DUR.ignite, b)));
  return clamp(GLOW_STEADY * near + (1 - GLOW_STEADY) * pulse);
}

/**
 * The three bands in flight at beat `b` (drawMorphChef's discs, over the
 * chef at `place`): each dips with the others, flies its route to hover
 * over its lobe, landing on its beat and glowing, its header riding it,
 * then falls with the toque and fades into the hat's tint as the hat
 * resolves.
 */
export function drawLobes(
  ctx: CanvasRenderingContext2D,
  b: number,
  bands: readonly Rect[],
  labels: readonly string[],
  place: Rect,
): void {
  const s = morphState(b);
  if (!(s.discs > 0 && b > 0)) return;
  const hover = HOVER * (1 - s.drop);
  const label = 1 - EASE.glide(progress(LABEL_OUT[0], LABEL_OUT[1], b));
  const discs = bands.slice(0, 3).map((band, i) => {
    const k = lobeFlight(i, b);
    const shape = lobeShape(i, b);
    const lobe = lobeAt(i, place);
    const a = { x: band.x + band.w / 2, y: band.y + band.h / 2 + s.dip };
    const to = { x: lobe.x, y: lobe.y - hover };
    const on = k > 0 ? curveAt(route(a, to, BULGE[i]), k) : a;
    const at = { x: on.x, y: on.y + lobeLift(i, b) };
    const w = lerp(band.w, 2 * lobe.r, shape);
    const h = lerp(band.h, 2 * lobe.r, k);
    return { band, i, k, shape, lobe, at, w, h };
  });
  // The glows first, under all three discs.
  for (const d of discs) {
    const g = lobeGlow(d.i, b) * s.discs;
    if (g > 0)
      glow(
        ctx,
        d.at.x,
        d.at.y,
        d.lobe.r + GLOW_REACH,
        PILLAR[lobePillar(d.i)],
        GLOW.ignite.alpha * g,
      );
  }
  for (const d of discs) {
    const pillar = lobePillar(d.i);
    // The disc rounds with its shape, not only its flight, so the three
    // read as three objects from their lift-off.
    const round = Math.max(d.k, 0.6 * d.shape);
    drawLobeDisc(
      ctx,
      { x: d.at.x - d.w / 2, y: d.at.y - d.h / 2, w: d.w, h: d.h },
      pillar,
      round,
      s.discs,
    );
    const text = labels[d.i];
    const la = label * s.discs;
    if (!text || la <= 0) continue;
    const f = font(TYPE.card.size, TYPE.card.header, MONO);
    const parts = labelParts(text);
    ctx.save();
    ctx.font = f;
    const wHead = ctx.measureText(parts.head).width;
    const wSub = ctx.measureText(parts.sub).width;
    const wClose = ctx.measureText(parts.close).width;
    ctx.restore();
    // The dotted part folding away (LABEL_FOLD): what is left of it, px.
    const fold = parts.sub ? EASE.move(clamp(d.shape / LABEL_FOLD)) : 1;
    const shown = wSub * (1 - fold);
    const tw = wHead + shown + wClose;
    // Where the card sets it (ART.md §7: 16 px inside the band, its em box
    // centred on the row), sliding to the disc's middle as the band rounds.
    const left = d.at.x - d.w / 2 + CARD_ART.inset - CARD_ART.bandInset;
    const x = lerp(left, d.at.x - tw / 2, EASE.move(d.shape));
    // Pillar on the dim band, ink on the lit disc, switched in one frame
    // where the fill under it is as far from one as from the other
    // (inkSwitch). A blend of the two passed through the fill's own tone:
    // each header vanished for a frame or two as its band lit.
    const dark = round >= inkSwitch(pillar);
    const y = d.at.y + 0.3 * TYPE.card.size;
    const style = { font: f, fill: dark ? COLOR.ink : PILLAR[pillar] };
    ctx.save();
    ctx.globalAlpha *= la * (dark ? LABEL_INK : 1);
    drawText(ctx, parts.head, x, y, style);
    if (shown > 0.5) {
      // Cut at the bracket as it slides over it, and fading.
      ctx.save();
      ctx.beginPath();
      ctx.rect(x + wHead, y - 1.2 * TYPE.card.size, shown, 2 * TYPE.card.size);
      ctx.clip();
      ctx.globalAlpha *= 1 - fold;
      drawText(ctx, parts.sub, x + wHead, y, style);
      ctx.restore();
    }
    if (parts.close) drawText(ctx, parts.close, x + wHead + shown, y, style);
    ctx.restore();
  }
}
