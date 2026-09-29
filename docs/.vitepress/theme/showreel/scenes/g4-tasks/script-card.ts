// args' file card: `mise-tasks/deploy` as the fixture has it, a file card
// (ART.md §8) whose `#USAGE` block folds like a card's table (§7): shut,
// it is one annotation row ("3 more lines"); unfolding, its rows fade in a
// sixteenth apart, rising 8 px, the rows under them and the card's own
// height following on the same curve. A seat mark can hold round the
// whole block, or round any one row.
//
// Its rows are set at the ledger's size (TYPE.ledger, 24 on 34), the other
// right-column file set smaller than the station card: at the file cards'
// 26 px the `choices` line, the one the reel ties `prod` to, would run 39
// columns into a 36-column card and lose "production" to the fade.

import { clamp, lerp } from "../../math";
import { type Rect, span } from "../../kit/motion";
import {
  annotation,
  type CardRows,
  fileFrame,
  gridRuns,
  missing,
  seatMark,
  syntaxFor,
} from "../../kit/parts";
import { DUR, FILE_ART, TYPE } from "../../kit/style";

/** The rows the card sets: mono 24 on a 34 px pitch. */
export const SCRIPT_TYPE = { size: TYPE.ledger.size, lineH: TYPE.ledger.lineH };
/** The card's bottom padding under its last row. */
const PAD = 18;
/** Unfolding rows rise this far as they fade in (ART.md §7). */
const RISE = 8;

export interface ScriptCardOptions {
  title: string;
  lines: readonly string[] | null;
  from: string;
  /** The block that folds: the rows it matches (consecutive). */
  fold: RegExp;
  /** Local beats, and when the block starts unfolding (Infinity: shut). */
  b: number;
  unfoldAt: number;
  /** A seat mark round the whole block, 0 to 1. */
  block?: number;
  /** Seat marks round single rows. */
  hl?: readonly { re: RegExp; a: number }[];
  alpha?: number;
}

/** The card's height with its block `k` of the way open. */
export function scriptHeight(
  lines: readonly string[],
  fold: RegExp,
  k: number,
): number {
  const n = lines.filter((l) => fold.test(l)).length;
  const rows = lines.length - n + lerp(1, n, k);
  return FILE_ART.tabH + FILE_ART.top + rows * SCRIPT_TYPE.lineH + PAD;
}

/** How far open the block is at beat `b` (DUR.fold, arrive). */
export const unfoldK = (b: number, at: number): number =>
  span(b, at, DUR.fold, "arrive");

/**
 * The card at `r` (its height set by the fold: `r.h` is ignored), with each
 * row's baseline and box returned, as `card` returns them.
 */
export function scriptCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: ScriptCardOptions,
): CardRows {
  const placed: CardRows = [];
  const alpha = o.alpha ?? 1;
  if (alpha <= 0) return placed;
  if (o.lines === null) {
    missing(ctx, { ...r, h: 240 }, o.from, alpha);
    return placed;
  }
  const { size, lineH } = SCRIPT_TYPE;
  const k = unfoldK(o.b, o.unfoldAt);
  const box = { ...r, h: scriptHeight(o.lines, o.fold, k) };
  const body = fileFrame(ctx, box, o.title, { alpha });
  const syntax = syntaxFor(o.title);
  const x = box.x + FILE_ART.inset;
  // Long lines fade out before the card's last 18 px, not re-wrapped, so
  // no faded glyph fragment sits on a highlight's outline (its right edge
  // is 12 px in); the 39-column `choices` line still ends clear of it.
  const fade = [box.x + box.w - 36, box.x + box.w - 18] as const;
  const first = o.lines.findIndex((l) => o.fold.test(l));
  const n = o.lines.filter((l) => o.fold.test(l)).length;
  ctx.save();
  ctx.globalAlpha *= alpha;
  ctx.beginPath();
  ctx.rect(box.x, body.y, box.w, Math.max(0, body.h - 2));
  ctx.clip();
  let y = body.y + FILE_ART.top;
  const rowBox = (top: number, h: number = lineH) => ({
    x: box.x + 12,
    y: top + 2,
    w: box.w - 24,
    h: h - 4,
  });
  // The block's mark, under its rows.
  const blockTop = body.y + FILE_ART.top + Math.max(0, first) * lineH;
  if ((o.block ?? 0) > 0 && first >= 0)
    seatMark(ctx, rowBox(blockTop, lerp(1, n, k) * lineH), o.block ?? 0);
  o.lines.forEach((line, i) => {
    const inBlock = i >= first && i < first + n && first >= 0;
    if (inBlock && i === first && k < 1) {
      // The shut block: its annotation, fading as the rows open.
      const pill = `${n} more lines`;
      annotation(ctx, pill, x, y + lineH / 2, clamp(1 - 2 * k));
    }
    if (inBlock) {
      // The block's rows fan down from its first slot as it opens.
      const j = i - first;
      const top = blockTop + j * lineH * k;
      const rk = span(o.b, o.unfoldAt + j * DUR.stagger, DUR.fold, "arrive");
      if (rk > 0) {
        const base = top + (lineH - size) / 2 + 0.8 * size;
        const hl = (o.hl ?? []).reduce(
          (m, h) => (h.re.test(line) ? Math.max(m, h.a) : m),
          0,
        );
        seatMark(ctx, rowBox(top), hl);
        ctx.save();
        ctx.globalAlpha *= rk;
        const w = gridRuns(ctx, syntax(line), x, base + RISE * (1 - rk), size, {
          fade,
        });
        ctx.restore();
        placed.push({ text: line, y: base, x, kind: "body", w, top, h: lineH });
      }
      if (j === n - 1) y = blockTop + lerp(1, n, k) * lineH;
      return;
    }
    const base = y + (lineH - size) / 2 + 0.8 * size;
    const hl = (o.hl ?? []).reduce(
      (m, h) => (h.re.test(line) ? Math.max(m, h.a) : m),
      0,
    );
    seatMark(ctx, rowBox(y), hl);
    const w = gridRuns(ctx, syntax(line), x, base, size, { fade });
    placed.push({ text: line, y: base, x, kind: "body", w, top: y, h: lineH });
    y += lineH;
  });
  ctx.restore();
  return placed;
}
