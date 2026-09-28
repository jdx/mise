// tools: the ticket that opens Dev tools (STORYBOARD.md Act I): `[tools]`,
// pink. The pitch's card (its three tables lit) and its terminal on `mise
// run ci`'s last line leave under the printing paper; as it lifts off the
// rail, use's stage comes in under it: the api card at `[tools]` and the
// terminal at `~/work/api $`.
//
// The ticket hands the stage on (STORYBOARD.md "Tickets hand the stage on";
// scenes/g1-brand/tickets.ts ticketScene): the inherited layers fall and
// fade from beat 0, a sixteenth apart, gone by 0.5; the ticket prints,
// tears on 3/4, swings and lifts off with the rail (kit/ticket.ts); the
// next stage rises in under it from 1.3, at rest by 1.85, and the bar
// line holds it.

import type { SceneCues } from "../score/cues";
import { TICKET_CUES, ticketScene } from "./g1-brand/tickets";

export const scene = ticketScene("tools");

/** The score's cues: the printer's steps, the tear, the lift-off. */
export const cues: SceneCues<"tools"> = TICKET_CUES;
