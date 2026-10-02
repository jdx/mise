// args: file tasks take `#USAGE` args, and bad input stops the task before
// it runs.
//
// The picture's cues: the `#USAGE` lines light with a pluck; `--help`
// prints with a light tick, as the reel's printed lines do; the `mise
// ERROR` lines are a dissonant brass cluster on the impact, the music
// ducked hard under it (props.ts errorStab); and the thread ties `prod` to
// the card's `choices` with a soft click.

import type { Part } from ".";
import { pizz } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { errorStab, PAN, seat } from "./props";
import { tick } from "./sounds";

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("args", s, facts);
    for (const t of c.all("usage")) pizz(m, t, hz(72), 0.06, PAN.card, 0.3);
    for (const t of c.all("help")) tick(m, t, 2400, 0.22, PAN.term, 0.1);
    for (const t of c.all("error")) errorStab(m, t, 1, PAN.term);
    for (const t of c.all("tie")) seat(m, t, 0.6, 0.1);
  },
};
