// The landing-page films: the source reel's scenes, played on the film
// clock edit.ts lays out, each at its recorded pace. Where the film cuts
// between two sections the source never put side by side, the scenes are
// joined here rather than dipped to black: a ticket takes down the stage of
// the section the film plays before it (bible.ts Joins, kit/grey.ts G.prev),
// so the bar line it starts from is the frame the film arrives on; any
// other cut leaves on the kit's exit (fall 16 px and fade over 3/8 beat)
// and enters on its entrance (rise 24 px and fade in over 1/2 beat), over
// the still stage (compose.ts Reel.render's `frame`). The source's own bar
// lines carry straight on. The tickets are numbered in the order the film
// plays them.
import { BEAT, type Join, type Joins, type ReelFacts, sec } from "./bible";
import type { Reel, ReelOptions } from "./compose";
import {
  continuous,
  type Cut,
  cuts,
  type Edition,
  filmChapters,
  filmDuration,
} from "./edit";
import { boundaryOut } from "./kit/grey";
import { enter, exit, type Presence } from "./kit/motion";
import { DUR } from "./kit/style";
import { createReel, POSTER_TIME } from "./reel";

const ROMAN = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX"];

/**
 * The joins a film's cuts need (bible.ts Joins). A ticket that follows a
 * section the source did not put before it inherits that section's own
 * last bar line, the frame its last moment holds (handoff.ts), when the
 * film plays the section to its end; the ticket takes that stage down. Each
 * ticket's act is numbered in film order.
 */
export function filmJoins(edition: Edition): Joins {
  const joins: Joins = {};
  const list = cuts(edition);
  let acts = 0;
  list.forEach((cut, k) => {
    if (!sec(cut.id).ticket) return;
    const join: Join = { meta: `ACT ${ROMAN[acts++]}` };
    const prev = list[k - 1];
    if (prev && !continuous(prev, cut)) {
      const out = boundaryOut(prev.id);
      if (out && prev.to === sec(prev.id).end) join.in = out;
    }
    joins[cut.id] = join;
  });
  return joins;
}

/** Whether cut `k` of `list` arrives on a cut the source did not make and no join hands on. */
const arrives = (list: readonly Cut[], k: number, joins: Joins): boolean =>
  k > 0 && !continuous(list[k - 1], list[k]) && !joins[list[k].id]?.in;

/**
 * How the scene of cut `k` stands at film time `time`: entering over its
 * first half beat after a cut the source did not make, leaving over the
 * 3/8 beat before the next; exactly on its mark otherwise, and on both
 * sides of every join a ticket makes.
 */
export function framePresence(
  list: readonly Cut[],
  k: number,
  time: number,
  joins: Joins,
): Presence {
  const cut = list[k];
  const b = (time - cut.start) / BEAT;
  const len = (cut.end - cut.start) / BEAT;
  const comes = arrives(list, k, joins);
  const goes = k + 1 < list.length && arrives(list, k + 1, joins);
  const i = comes ? enter(b, 0) : { alpha: 1, dy: 0 };
  const e = goes ? exit(b, len - DUR.exit) : { alpha: 1, dy: 0 };
  return { alpha: i.alpha * e.alpha, dy: i.dy + e.dy };
}

/** Render an edition on film time while replaying scenes on their source clock. */
export function createFilm(
  facts: ReelFacts | null,
  edition: Edition = "tour",
  options: ReelOptions = {},
): Reel {
  const joins = filmJoins(edition);
  const source = createReel(facts, { ...options, joins });
  const list = cuts(edition);
  return {
    duration: filmDuration(edition),
    chapters: filmChapters(edition),
    render(ctx, time, pw, ph) {
      let k = list.findIndex((cut) => time < cut.end);
      if (k < 0) k = list.length - 1;
      const cut = list[k];
      const local = Math.max(
        0,
        Math.min(cut.end - cut.start - 1e-6, time - cut.start),
      );
      source.render(
        ctx,
        cut.from + local,
        pw,
        ph,
        framePresence(list, k, cut.start + local, joins),
      );
    },
  };
}

/**
 * The poster the player shows until someone presses play: the source
 * reel's poster frame (reel.ts POSTER_TIME, the switch with both versions
 * on their folder cards and the caption up), without a slate.
 */
export function drawFilmPoster(
  ctx: CanvasRenderingContext2D,
  pw: number,
  ph: number,
  facts: ReelFacts | null,
): void {
  createReel(facts).render(ctx, POSTER_TIME, pw, ph);
}
