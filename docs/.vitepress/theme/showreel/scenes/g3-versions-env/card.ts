// The station card moves the Versions and Environments scenes share
// (ART.md §7): a seated value flipping to a new one (packslip's hk), and
// the card folding away into its tab strip and back (redact). Each draws
// kit/parts.ts card() and is exactly card() at rest.

import { gridRuns, splitRuns, tomlRuns } from "../../kit/parts";
import { card, type CardOptions, type CardRows } from "../../kit/grey";
import { type Rect, span } from "../../kit/motion";
import { CARD_ART, DUR, TYPE } from "../../kit/style";
import { clamp } from "../../math";

/**
 * The card with `line` (a regex for one of its rows) flipping from its
 * value in `before` to its value in `after` at beat `at` (ART.md §7 Flip):
 * the row's key stays, and its value turns like a counter's wheel over
 * 1/4 beat (move, no overshoot): the old value rolls up out of the row as
 * the new one rolls up into it from below, one row's height apart and both
 * clipped to the row, the old fading over the first 60 % and the new in
 * over the last 60 %, so the two are never drawn over each other. Before
 * `at` the card is `before`; from `at + 1/4` it is `after`, exactly card()
 * of it.
 */
export function flipCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: Omit<CardOptions, "text"> & {
    before: string | null;
    after: string | null;
    line: RegExp;
    b: number;
    at: number;
  },
): CardRows {
  const { before, after, line, b, at, ...rest } = o;
  if (b < at) return card(ctx, r, { ...rest, text: before });
  if (b >= at + DUR.tick || before === null || after === null)
    return card(ctx, r, { ...rest, text: after });
  const find = (t: string) => t.split("\n").find((l) => line.test(l)) ?? null;
  const was = find(before);
  const now = find(after);
  if (was === null || now === null)
    return card(ctx, r, { ...rest, text: after });
  // The row's key stays; only its value moves.
  const valueAt = Array.from(/^[^=]*=\s*/.exec(now)?.[0] ?? "").length;
  const keyOnly = Array.from(now).slice(0, valueAt).join("");
  const text = after
    .split("\n")
    .map((l) => (l === now ? keyOnly : l))
    .join("\n");
  const rows = card(ctx, r, { ...rest, text });
  const row = rows.find((x) => x.kind === "body" && line.test(x.text));
  if (!row || row.top === undefined || row.h === undefined) return rows;
  const size = o.size ?? TYPE.card.size;
  const x = row.x + valueAt * 0.6 * size;
  const k = span(b, at, DUR.tick, "move");
  const fade = [r.x + r.w - CARD_ART.inset - 24, r.x + r.w - 12] as const;
  const draw = (l: string, dy: number, a: number) => {
    if (a <= 0) return;
    ctx.save();
    ctx.globalAlpha *= a;
    gridRuns(ctx, splitRuns(tomlRuns(l), valueAt)[1], x, row.y + dy, size, {
      fade,
    });
    ctx.restore();
  };
  ctx.save();
  ctx.globalAlpha *= o.alpha ?? 1;
  // The row's own box: the values never leave it.
  ctx.beginPath();
  ctx.rect(r.x, row.top, r.w, row.h);
  ctx.clip();
  draw(was, -row.h * k, 1 - clamp(k / 0.6));
  draw(now, row.h * (1 - k), clamp((k - 0.4) / 0.6));
  ctx.restore();
  return rows;
}

/**
 * The card folded `k` of the way into its tab strip (0 open, 1 only the
 * strip): its body clipping upward, rows below the fold gone, and the
 * strip fading over the last quarter (redact's "folds away").
 */
export function foldedCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: CardOptions,
  k: number,
): CardRows {
  if (k <= 0) return card(ctx, r, o);
  const h = r.h - (r.h - CARD_ART.tabH) * Math.min(1, k);
  const a = 1 - Math.max(0, (k - 0.75) / 0.25);
  if (a <= 0) return [];
  return card(ctx, { ...r, h }, { ...o, alpha: (o.alpha ?? 1) * a });
}
