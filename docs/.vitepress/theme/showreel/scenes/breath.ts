// breath (STORYBOARD.md Act VII; ART.md §3 "Dim"; STORYBOARD.md "Pacing"):
// the valley before the climax, near silence, one motion at a time. The
// lights go down over bootstrap's last frame, held in the same terminal
// (kit/rest.ts bootstrap|breath). In the dark a ⌃L keycap comes up on the
// pane, the one lit thing on the stage, and on its press the take's real
// Ctrl-L clears that same shell, never activated: C19 opens on the screen
// it cleared (`~ $`), and the illustration badge bootstrap's `you/setup`
// put up fades over a quarter beat from the press (ART.md §9: badges leave
// by fading). Then the lights come back up as the cloned project's card
// rises in, unlit, onto the stage the clone starts from (breath|clone),
// which holds still into the bar line. The terminal holds through both
// bar lines.

import { capture } from "../captures";
import {
  dimLights,
  greyScene,
  keep,
  keycap,
  keycapOn,
  LEFT,
  rest,
} from "../kit/grey";
import { enter, span } from "../kit/motion";
import { BADGES, drawPaneWindow, drawScreen, OPENS } from "../kit/rest";
import { Pace } from "../kit/pace";
import { DUR } from "../kit/style";
import type { SceneCues } from "../score/cues";

/** The keycap's whole life (kit/parts.ts keycap): up over DUR.keycapLead to the press, gone 1.5 beats after it. */
const KEYCAP_LIFE = DUR.keycapLead + 1.5;

/**
 * When everything happens (kit/pace.ts): the lights down from the bar
 * line; the ⌃L keycap coming up, pressed (the take's Ctrl-L clears the
 * screen on the press), let go and gone; then the lights up as the card
 * rises in, and the stage holds still into the bar line.
 */
const schedule = (() => {
  const p = new Pace("breath", { from: 0 });
  const dim = p.move("dim", DUR.std);
  const clear = p.move("clear", KEYCAP_LIFE);
  const up = p.move("up", DUR.std);
  return { p, dim, up, press: clear.at + DUR.keycapLead };
})();

export const cues: SceneCues<"breath"> = { clear: schedule.press };

/** The schedule, for the pacing checks. */
export const breathSchedule = () => schedule;

export const scene = greyScene(
  "breath",
  (g) => {
    const { ctx, b } = g;
    const { press, dim, up } = schedule;
    // The cloned project's card (breath|clone's first layer), rising in as
    // the lights come up.
    const card = enter(b, up.at);
    if (card.alpha > 0)
      keep(g, () => {
        ctx.save();
        ctx.translate(0, card.dy);
        rest(g, "breath|clone", card.alpha, [0]);
        ctx.restore();
      });
    // The one shell: bootstrap's last frame, until the ⌃L clears it to C19's
    // first; the illustration badge fades out over the quarter beat after the
    // press, then the pane is breath|clone's own rest.
    const c = capture(g.d, "C19");
    const badge = 1 - span(b, press, DUR.badge, "glide");
    if (b < press) rest(g, "bootstrap|breath");
    else if (c && badge > 0)
      keep(g, () => {
        drawPaneWindow(ctx, LEFT);
        drawScreen(ctx, LEFT, c, OPENS.C19!(c), g.t, {
          badges: [{ text: BADGES.illustration, alpha: badge }],
        });
      });
    else rest(g, "breath|clone", 1, [1]);
    dimLights(
      ctx,
      span(b, dim.at, dim.end - dim.at, "glide") *
        (1 - span(b, up.at, up.end - up.at, "glide")),
    );
    // The key, over the dark: pressed on the take's own Ctrl-L.
    const k = keycapOn(LEFT, "ctrl-l");
    keycap(ctx, k.x, k.y, "ctrl-l", b, press);
  },
  { events: () => schedule.p.events() },
);
