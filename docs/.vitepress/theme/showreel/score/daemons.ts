// daemons: a task declares the daemons it needs, and `mise run db` starts
// Postgres and waits until it is ready.
//
// The picture's cues: `cd ../shop` ticks; `✔ … started` is the pilot
// light's whump; and the thread tying the server's version to
// `postgres = "18"` clicks.

import type { Part } from ".";
import { listen } from "./listen";
import { PAN, seat, whump } from "./props";
import { tick } from "./sounds";

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("daemons", s, facts);
    for (const t of c.all("cd")) tick(m, t, 2600, 0.32, PAN.term, 0.1);
    for (const t of c.all("started")) whump(m, t, 1, PAN.card);
    for (const t of c.all("tie")) seat(m, t, 0.7);
  },
};
