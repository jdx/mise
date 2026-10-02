// morph (STORYBOARD.md Act VIII, ART.md §11 Morph): the climax's card,
// folded to its three lit headers, holds from clone. On the downbeat the
// three lit header bands dip and lift off it, tools, env, tasks an eighth
// apart, each carrying its header and rounding into a disc
// as it flies its own route (none crossing another) to hover over the
// chef's head, while the card body folds up into its tab strip and the
// panel fades. The chef below the hat rises in big and centred (1.3×,
// scenes/g1-brand/morph-card.ts MORPH_BIG), not in its end-card corner:
// the brand's climax takes the stage. The discs land on beats 2, 2.5 and
// 3, each igniting in its pillar as it lands, their headers riding them,
// and hover over the bare head, floating a little; they wind up, and in
// bar 2 (beat 4) drop onto it as the toque and resolve into the vector
// hat, keeping their tint. Over beats 5 to 6.5 the chef glides to its
// end-card place as the tint drains to paper. Then nothing moves but the
// motes, into the end card. The chef at rest is the morph|end frame
// (kit/rest.ts chefRest).
//
// Everything here is kept (drawn out from under the section's edge fade):
// the card is the bar line's at rest on the first frame, the discs are its
// bands from the first frame after, and the chef is the next bar line's.

import { fileOf } from "../captures";
import { chefLit, drawMorphChef, MORPH_CUE, morphState } from "../kit/chef";
import {
  annotation,
  card,
  type CardRows,
  greyScene,
  keep,
  rest,
} from "../kit/grey";
import type { Rect } from "../kit/motion";
import { endCardRects } from "../kit/namecard";
import { restOf } from "../kit/rest";
import { CARD_ART, DUR, EASE, MOTION, TYPE } from "../kit/style";
import { lerp, progress } from "../math";
import type { SceneCues } from "../score/cues";
import {
  drawLobes,
  foldedNotes,
  morphPlace,
  pillAt,
} from "./g1-brand/morph-card";

/** The bar line the morph starts from: the compact card, its three headers lit. */
const FROM = "clone|morph" as const;
const REST = restOf(FROM)[0];
if (REST.kind !== "card") throw new Error(`${FROM} starts on a card`);
const SPEC = REST;

/** The three lit headers, in the card's reading order: they become lobes 0, 1, 2. */
const HEADS = [/^\[tools\]/, /^\[env\]/, /^\[tasks/] as const;

// The card's own moves (ART.md §11 Morph "0 → 1").

/**
 * Its body folds up into the tab strip as the bands fly clear of it (move,
 * from their lift over 3/4 beat, so no band is left hanging below its edge),
 * and the panel fades from 1/4 beat (glide: leave's late rush flickers a
 * panel this big at 30 fps).
 */
const FOLD = [MORPH_CUE.fly, MORPH_CUE.fly + DUR.move] as const;
const FADE = [DUR.tick, DUR.tick + DUR.half] as const;
/** The annotation pills ("4 lines") fade as their headers lift, riding the dip. */
const PILLS = [0, (3 / 2) * DUR.flick] as const;

/** A frame's card options, as the bar line draws the card. */
const cardOptions = (text: string | null) => ({
  title: SPEC.title,
  text,
  from: SPEC.from,
  open: SPEC.open,
  hide: SPEC.hide,
  size: SPEC.size,
});

/** Where the card sets its rows (drawn under an empty clip: nothing reaches the frame). */
function rowsOf(ctx: CanvasRenderingContext2D, text: string | null): CardRows {
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, 0, 0);
  ctx.clip();
  const rows = card(ctx, SPEC.rect, { ...cardOptions(text), lit: SPEC.lit });
  ctx.restore();
  return rows;
}

/** Each lit header's band, as the card paints it (ART.md §7 Header lighting). */
const bandOf = (row: CardRows[number]): Rect => ({
  x: SPEC.rect.x + CARD_ART.bandInset,
  y: row.top ?? row.y - 0.8 * TYPE.card.size,
  w: SPEC.rect.w - 2 * CARD_ART.bandInset,
  h: row.h ?? TYPE.card.lineH,
});

export const scene = greyScene(
  "morph",
  (g) => {
    const { ctx, b } = g;
    keep(g, () => {
      if (b <= 0) {
        rest(g, FROM);
        return;
      }
      const text = fileOf(g.d, SPEC.from, SPEC.path);
      const rows = rowsOf(ctx, text);
      const heads = HEADS.map((re) =>
        rows.find((r) => r.kind === "header" && re.test(r.text)),
      );
      const bands = heads.map((r) => (r ? bandOf(r) : null));
      // The card, folding up into its tab strip and fading as the bands lift.
      const fold = EASE.move(progress(FOLD[0], FOLD[1], b));
      const alpha = 1 - EASE.glide(progress(FADE[0], FADE[1], b));
      const rect = {
        ...SPEC.rect,
        h: lerp(SPEC.rect.h, CARD_ART.tabH, fold),
      };
      // Its frame alone (no rows written on): the headers ride the bands
      // and the pills are drawn over them below, so the card's surface
      // stays whole under the bands as they narrow and leave.
      if (alpha > 0)
        card(ctx, rect, { ...cardOptions(text), lit: {}, reveal: 0, alpha });
      // The chef below the hat comes up under the lifting bands as a
      // window does (rising MOTION.riseIn px on its own fade's arrive,
      // not only fading: a paper chef at half alpha read as a grey ghost),
      // then takes the hat; at rest from MORPH_CUE.still. The rise is 0
      // once the body is up, long before the toque drops.
      const rise = MOTION.riseIn * (1 - morphState(b).body);
      const place = morphPlace(b);
      ctx.save();
      ctx.translate(0, rise);
      drawMorphChef(ctx, b, {
        bands: [],
        t: g.t,
        place,
        avoid: endCardRects(ctx),
      });
      ctx.restore();
      const found = bands.filter((x): x is Rect => x !== null);
      if (found.length === 3)
        drawLobes(
          ctx,
          b,
          found,
          heads.map((r) => r?.text ?? ""),
          place,
        );
      // Each header's annotation pill rides its band's dip and fades as
      // the band lifts, over the disc it sat on.
      const pill = 1 - EASE.glide(progress(PILLS[0], PILLS[1], b));
      if (pill > 0 && text) {
        const notes = foldedNotes(text);
        const dip = morphState(b).dip;
        heads.forEach((r) => {
          const note = r ? notes.get(r.text) : undefined;
          if (!r || !note) return;
          const p = pillAt(r);
          annotation(ctx, note, p.x, p.midY + dip, pill);
        });
      }
    });
  },
  {
    // The chef as it fades up under the lobes, where it stands: none on
    // the first frame (clone|morph keeps no screen), morph|end's once it
    // has glided to its place.
    lit: (b) => {
      const a = morphState(b).body;
      return a > 0 ? chefLit(morphPlace(b), a) : null;
    },
  },
);

/**
 * The score's cues (score/cues.ts LISTEN.morph): the bands lift, each
 * settles over its lobe (drawLobes' flights end exactly on
 * MORPH_CUE.lands, glide), the toque drops and touches the head.
 */
export const cues: SceneCues<"morph"> = {
  lift: MORPH_CUE.fly,
  lands: MORPH_CUE.lands,
  drop: MORPH_CUE.drop,
  land: MORPH_CUE.land,
};
