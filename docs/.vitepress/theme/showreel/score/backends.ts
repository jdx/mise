// backends: `npm:` types into its slot, prettier installs and checks, then
// the github backend and the others.
//
// The picture's cues: `npm:` types in on four pizzicato plucks up F minor
// in tools pink; `"npm:prettier"` seats with the click; ⌃L; prettier's
// verdict is a pass, the kitchen's bell, and its file chips tick; `mise use
// github:cli/cli` prints its tools line on a lighter click; and the
// `pypi:`, `cargo:` and `go:` chips land on three plucks up the F minor
// chord the bed plays there.

import type { Part } from ".";
import { pizz } from "./instruments";
import { listen } from "./listen";
import { hz } from "./mix";
import { PAN, keyClick, seat, serviceBell } from "./props";
import { tick } from "./sounds";

/** n, p, m, : climb F minor: C5 F5 A♭5 C6. */
const NPM = [72, 77, 80, 84];
/** The chips climb F minor, the bed's chord as they land: A♭5 C6 F6. */
const CHIPS = [80, 84, 89];

export const part: Part = {
  cues(m, s, facts) {
    const c = listen("backends", s, facts);
    c.all("npm").forEach((t, i) =>
      pizz(m, t, hz(NPM[i % 4]), 0.075, -0.1 + 0.05 * i, 0.25, { bright: 9 }),
    );
    for (const t of c.all("seat")) seat(m, t);
    for (const t of c.all("clear")) keyClick(m, t);
    for (const t of c.all("verdict")) serviceBell(m, t, 0.035, PAN.term);
    for (const t of c.all("chips")) tick(m, t, 3000, 0.22, PAN.card, 0.12);
    for (const t of c.all("gh")) seat(m, t, 0.7, PAN.term);
    c.all("backends").forEach((t, i) =>
      pizz(m, t, hz(CHIPS[i % 3]), 0.07, -0.45 + 0.15 * i, 0.3, { len: 0.6 }),
    );
  },
};
