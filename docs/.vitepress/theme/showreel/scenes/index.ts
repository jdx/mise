// Every section's scene, in timeline order, each drawn with the kit
// (kit/grey.ts greyScene). Each lives in its own module named for its section id,
// exporting `scene`; showreel-frames.mjs --section bundles that module alone.

import type { Scene } from "../bible";
import { scene as args } from "./args";
import { scene as backends } from "./backends";
import { scene as bootstrap } from "./bootstrap";
import { scene as breath } from "./breath";
import { scene as clone } from "./clone";
import { scene as daemons } from "./daemons";
import { scene as depends } from "./depends";
import { scene as dotfiles } from "./dotfiles";
import { scene as end } from "./end";
import { scene as env } from "./env";
import { scene as lock } from "./lock";
import { scene as machines } from "./machines";
import { scene as morph } from "./morph";
import { scene as newMachine } from "./new";
import { scene as open } from "./open";
import { scene as packslip } from "./packslip";
import { scene as pitch } from "./pitch";
import { scene as redact } from "./redact";
import { scene as registry } from "./registry";
import { scene as skip } from "./skip";
import { scene as switchScene } from "./switch";
import { scene as tasks } from "./tasks";
import { scene as tools } from "./tools";
import { scene as track } from "./track";
import { scene as use } from "./use";
import { scene as vars } from "./vars";
import { scene as versions } from "./versions";

export const scenes: Scene[] = [
  open,
  pitch,
  tools,
  use,
  registry,
  backends,
  versions,
  switchScene,
  packslip,
  env,
  vars,
  redact,
  tasks,
  depends,
  skip,
  args,
  daemons,
  dotfiles,
  track,
  machines,
  lock,
  newMachine,
  bootstrap,
  breath,
  clone,
  morph,
  end,
];
