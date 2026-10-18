// What the Versions and Environments scenes read from their bar lines'
// rests (kit/rest.ts RESTS): the pane a bar line keeps (its preset, and the
// screen crop it holds it with), and the station card it keeps, if any, as
// card() options. A scene that draws its own terminal and card from these
// meets its bar lines exactly, whatever preset, fit or crop the rest uses,
// so a change there (a larger pane, a fitted card) is one edit in
// kit/rest.ts, not one per scene.

import { fileOf, type ReelData } from "../../captures";
import type { BoundaryId } from "../../handoff";
import { CARD, type CardOptions } from "../../kit/parts";
import { type Layer, restOf } from "../../kit/rest";

export type CardLayer = Extract<Layer, { kind: "card" }>;
export type PaneLayer = Extract<Layer, { kind: "pane" }>;

/** Bar line `id`'s pane layer. Every bar line these scenes share keeps one. */
export function restPane(id: BoundaryId): PaneLayer {
  const l = restOf(id).find((x): x is PaneLayer => x.kind === "pane");
  if (!l) throw new Error(`${id} keeps no terminal`);
  return l;
}

/** Bar line `id`'s station card layer, or null when it keeps none. */
export const restCard = (id: BoundaryId): CardLayer | null =>
  restOf(id).find((x): x is CardLayer => x.kind === "card") ?? null;

/** A card layer as card() options: exactly what the rest draws (kit/rest.ts drawLayer). */
export const cardOptions = (d: ReelData | null, l: CardLayer): CardOptions => ({
  title: l.title,
  text: fileOf(d, l.from, l.path),
  from: l.from,
  open: l.open,
  hide: l.hide,
  lit: l.lit,
  size: l.size,
  fit: l.fit,
});

/**
 * The station card at switch|packslip: the card switch's api/ folder
 * grows into and packslip opens on. The bar line's own card when it keeps
 * one (STATION_KEPT); until it does, the one it would keep: C7's file, as
 * the folder showed it, [tools] open, where the card stands.
 */
const kept = restCard("switch|packslip");
export const STATION_KEPT = kept !== null;
export const STATION: CardLayer = kept ?? {
  kind: "card",
  rect: CARD,
  title: "~/work/api/mise.toml",
  from: "C7",
  path: "work/api/mise.toml",
  open: ["[tools]"],
};
