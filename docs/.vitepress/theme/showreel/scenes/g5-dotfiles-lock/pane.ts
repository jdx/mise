// The terminal as track and lock keep it: one window up across both bar
// lines, holding a take's real screen at the take time the plays map the
// beat to, drawn with the same calls the bar lines make (kit/rest.ts
// drawPaneWindow, drawScreen), so the first and last frames are the
// rests'. On top of kit/grey.ts shot it adds what these two need: the
// pane's own dim (PANE_DIM, which a shot's alpha multiplier is not), and
// rows dimmed by what they say (a table while the diagram carries it).
// Returns the screen and where its rows are, for a copy lifting out of one.

import {
  type Capture,
  type CaptureId,
  capture,
  screenAt,
  type Screen,
  type ScreenOptions,
} from "../../captures";
import {
  clipTime,
  type G,
  keep,
  missing,
  type Play,
  rectOf,
} from "../../kit/grey";
import { drawPaneWindow, drawScreen } from "../../kit/rest";
import type { Pane, TermLayout } from "../../kit/term";

export interface TakePaneOptions {
  /** 0 to 1: the whole pane, window and text (an entrance or an exit). */
  alpha?: number;
  /** 0 to 1: the pane dimmed (PANE_DIM), its body, text and title. */
  dim?: number;
  /** Each shown line's alpha, by its text. */
  lineAlpha?: (i: number, lines: readonly string[]) => number;
  /** Offset the whole window, px (an exit's fall). */
  dy?: number;
  /** Screen crops (captures.ts ScreenOptions): soft wraps joined to their line, cropped with it. */
  screen?: ScreenOptions;
}

export interface TakePane {
  c: Capture;
  ct: number;
  screen: Screen;
  layout: TermLayout;
}

/**
 * Take `id` in pane `p`, kept (it holds through the bar lines), its
 * screen at the take time `plays` map beat `g.b` to. Without the take, the
 * window and the missing card, as the rests draw them.
 */
export function takePane(
  g: G,
  id: CaptureId,
  p: Pane,
  plays: (c: Capture) => readonly Play[],
  o: TakePaneOptions = {},
): TakePane | null {
  const alpha = o.alpha ?? 1;
  if (alpha <= 0) return null;
  const c = capture(g.d, id);
  return keep(g, () => {
    const { ctx } = g;
    ctx.save();
    ctx.globalAlpha *= alpha;
    if (o.dy) ctx.translate(0, o.dy);
    drawPaneWindow(ctx, p, o.dim ?? 0);
    let out: TakePane | null = null;
    if (!c) missing(ctx, rectOf(p), id);
    else {
      const { ct } = clipTime(plays(c), g.b);
      const layout = drawScreen(ctx, p, c, ct, g.t, {
        dim: o.dim ?? 0,
        lineAlpha: o.lineAlpha,
        screen: o.screen,
      });
      out = { c, ct, screen: screenAt(c, ct, o.screen), layout };
    }
    ctx.restore();
    return out;
  });
}
