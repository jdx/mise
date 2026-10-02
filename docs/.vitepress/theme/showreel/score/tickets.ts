// The tickets: each act opens on one 2/4 bar, a kitchen order ticket that
// prints on the rail, tears, hangs and lifts away (ART.md §10). The sound
// follows the paper: a printer tick for each of the feed's twelve steps
// (kit/style.ts TICKET), the tear, and air as it lifts.
//
// A ticket scene may export `print`, `tear` and `lift` (score/cues.ts).

import type { Part } from ".";
import { TICKET } from "../kit/style";
import { listen } from "./listen";
import { air, feed, PAN, tear } from "./props";

type TicketId =
  "tools" | "versions" | "env" | "tasks" | "dotfiles" | "machines" | "new";

interface Spec {
  /** Where the ticket hangs: its stereo position. */
  pan: number;
  /** The bed's fader under it (score/index.ts Part.level), 1 by default. */
  level?: number;
}

const SPECS: Record<TicketId, Spec> = {
  tools: { pan: PAN.center },
  versions: { pan: PAN.center },
  // Act III's bed is its breakdown, lifted with the act (vars, redact).
  env: { pan: PAN.center, level: 1.4 },
  tasks: { pan: PAN.center },
  dotfiles: { pan: PAN.center },
  machines: { pan: PAN.center },
  // The new machine's ticket opens the valley, off to the right; the bed
  // rests under it (bed.toml), for the whip's riser.
  new: { pan: 0.52 },
};

function ticket(id: TicketId): Part {
  const spec = SPECS[id];
  return {
    level: spec.level,
    cues(m, s, facts) {
      const c = listen(id, s, facts);
      const torn = c.or("tear", TICKET.cue.tear);
      const lift = c.at("lift");
      feed(m, c.or("print", 0), torn, TICKET.feedSteps, 0.12, spec.pan);
      tear(m, torn, 0.1, spec.pan);
      if (lift !== null)
        air(m, lift, lift + s.len * 0.3, 0.1, spec.pan, spec.pan, true);
    },
  };
}

export const TICKETS: Record<TicketId, Part> = {
  tools: ticket("tools"),
  versions: ticket("versions"),
  env: ticket("env"),
  tasks: ticket("tasks"),
  dotfiles: ticket("dotfiles"),
  machines: ticket("machines"),
  new: ticket("new"),
};
