// Pitch: one mise.toml per project.
//
// The picture's cues: the card's three tables light in turn, each on its
// act's voice, F A♭ C up the tonic chord (a pizzicato for [tools], an organ
// for [env], a woodblock and a marimba for [tasks]); the cwd dot hops on
// `cd api` with a tick (the switch's `cd`s are the pitched ones); `mise
// run ci` prints its tasks' lines as a ripple, lint, test, build and a ding
// for ci; and the version and env var click home on the card rows that
// asked for them.

import type { Part } from ".";
import { marimba, organ, pizz, woodblock } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { PAN, seat } from "./props";
import { ding, tick } from "./sounds";

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("pitch", s, facts);
    const [tools, env, tasks] = c.all("tables");
    if (tools !== undefined)
      pizz(m, tools, hz(77), 0.08, PAN.card, 0.3, { len: 0.6 });
    if (env !== undefined)
      organ(m, env, env + 0.22, [80], 0.05, {
        speed: "fast",
        release: 0.2,
        sub: 0,
      });
    if (tasks !== undefined) {
      woodblock(m, tasks, 1250, 0.12, PAN.card, 0.12);
      marimba(m, tasks + 0.004, hz(84), 0.09, PAN.card);
    }
    for (const t of c.all("cd")) tick(m, t, 2600, 0.32, PAN.term, 0.1);
    for (const t of c.all("ci")) {
      woodblock(m, t, 1250, 0.07, PAN.term - 0.1, 0.1);
      pizz(m, t + 0.05, hz(72), 0.05, PAN.term, 0.2);
      marimba(m, t + 0.1, hz(77), 0.06, PAN.term + 0.1);
      ding(m, t + 0.15, hz(84), 0.025, PAN.term, 1, 0.35);
    }
    for (const t of c.all("lands")) seat(m, t, 1.2);
  },
};
