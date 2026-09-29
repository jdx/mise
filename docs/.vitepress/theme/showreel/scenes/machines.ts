// machines: the ticket that opens Everywhere (STORYBOARD.md Act VI):
// `mise.lock`, terracotta. Track's `~/.zshrc` card, its rail with the
// three checkpoints and the terminal on the rollback's last line leave
// under the printing paper; as it lifts off the rail, the api card
// returns as C17 left it (at `[tools]`) and the terminal comes up at
// `~/work/api $` for lock.
//
// The ticket hands the stage on (STORYBOARD.md "Tickets hand the stage on";
// scenes/g1-brand/tickets.ts ticketScene): the inherited layers fall and
// fade from beat 0, a sixteenth apart, gone by 0.5; the ticket prints,
// tears on 3/4, swings and lifts off with the rail (kit/ticket.ts); the
// next stage rises in under it from 1.3, at rest by 1.85, and the bar
// line holds it.

import type { SceneCues } from "../score/cues";
import { TICKET_CUES, ticketScene } from "./g1-brand/tickets";

export const scene = ticketScene("machines");

/** The score's cues: the printer's steps, the tear, the lift-off. */
export const cues: SceneCues<"machines"> = TICKET_CUES;
