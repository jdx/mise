// dotfiles: the ticket that opens Dotfiles (STORYBOARD.md Act V):
// `[dotfiles]`, terracotta. It moves the stage its own way inside the
// ticket's two windows (STORYBOARD.md "Tickets hand the stage on"): the
// shop card, held from daemons with its pilot light on, slides out right
// 60 px and fades under the printing paper, as a card whose file changes
// does (ART.md §7 "Title change"), while daemons' terminal, held on the
// server's version, falls and fades as every ticket's stage does
// (clearPresence); as the ticket lifts off the rail, the
// `~/.zshrc` file card (the fixture: the prompt, compinit and
// `eval "$(mise activate zsh)"`) slides in 60 px from the right (ART.md §8
// "Enter"), and the empty checkpoint rail under it and the terminal at
// `~ $` for track rise in after it, a sixteenth apart, at rest by 1.85.
//
// The stage it takes down is the bar line it starts from in the reel being
// drawn (kit/grey.ts G.prev): daemons|dotfiles in the source reel; in a
// film that plays skip before it (film.ts), skip|args, whose card and
// terminal leave the same way.

import { greyScene, keep, rest, stagePresence } from "../kit/grey";
import { restOf } from "../kit/rest";
import { drawTicketFor } from "../kit/ticket";
import { CARD_ART, EASE, MOTION, TICKET } from "../kit/style";
import { progress } from "../math";
import type { SceneCues } from "../score/cues";
import { clearPresence, TICKET_CUES, ticketLit } from "./g1-brand/tickets";

/**
 * The shop card slides out under the printing paper, gone by the tear's
 * clear (glide: see scenes/g1-brand/tickets.ts on why not leave).
 */
const OUT = TICKET.cue.clear;
const OUT_DUR = TICKET.cue.clearEnd - TICKET.cue.clear;

export const scene = greyScene(
  "dotfiles",
  (g) => {
    const { ctx, b } = g;
    const out = EASE.glide(progress(OUT, OUT + OUT_DUR, b));
    const { prev, next } = g;
    if (prev) {
      const down = clearPresence(prev, b);
      restOf(prev).forEach((l, i) => {
        // The card slides out right; anything else (the terminal) falls.
        const card = l.kind === "card";
        const a = card ? 1 - out : down[i].alpha;
        if (a <= 0) return;
        keep(g, () => {
          ctx.save();
          if (card) ctx.translate(CARD_ART.swap * out, 0);
          else ctx.translate(0, down[i].dy);
          rest(g, prev, a, [i]);
          ctx.restore();
        });
      });
    }
    // track's stage on the ticket's own entrance curve: the file card from
    // the right, the rail and the terminal rising.
    if (next)
      stagePresence(next, "enter", b).forEach((p, i) => {
        if (p.alpha <= 0) return;
        const k = p.dy / MOTION.riseIn;
        keep(g, () => {
          ctx.save();
          if (i === 0) ctx.translate(MOTION.slideIn * k, 0);
          else ctx.translate(0, p.dy);
          rest(g, next, p.alpha, [i]);
          ctx.restore();
        });
      });
    drawTicketFor(ctx, "dotfiles", b, { meta: g.join?.meta });
  },
  { lit: (b, _d, joins) => ticketLit("dotfiles", b, joins) },
);

/** The score's cues: the printer's steps, the tear, the lift-off. */
export const cues: SceneCues<"dotfiles"> = TICKET_CUES;
