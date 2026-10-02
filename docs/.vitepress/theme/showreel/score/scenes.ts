// Every scene module, by section id, for the cue API (score/cues.ts): the
// score reads a module's `cues` export where it has one. The modules are
// the picture's own, which the renderer bundles anyway; a scene imports
// from score/ only the cue types, so there is no cycle.

import * as args from "../scenes/args";
import * as backends from "../scenes/backends";
import * as bootstrap from "../scenes/bootstrap";
import * as breath from "../scenes/breath";
import * as clone from "../scenes/clone";
import * as daemons from "../scenes/daemons";
import * as depends from "../scenes/depends";
import * as dotfiles from "../scenes/dotfiles";
import * as end from "../scenes/end";
import * as env from "../scenes/env";
import * as lock from "../scenes/lock";
import * as machines from "../scenes/machines";
import * as morph from "../scenes/morph";
import * as newMachine from "../scenes/new";
import * as open from "../scenes/open";
import * as packslip from "../scenes/packslip";
import * as pitch from "../scenes/pitch";
import * as redact from "../scenes/redact";
import * as registry from "../scenes/registry";
import * as skip from "../scenes/skip";
import * as switchScene from "../scenes/switch";
import * as tasks from "../scenes/tasks";
import * as tools from "../scenes/tools";
import * as track from "../scenes/track";
import * as use from "../scenes/use";
import * as vars from "../scenes/vars";
import * as versions from "../scenes/versions";
import type { SectionId } from "../timeline";

export const SCENE_MODULES: Readonly<Record<SectionId, object>> = {
  open,
  pitch,
  tools,
  use,
  registry,
  backends,
  versions,
  switch: switchScene,
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
  new: newMachine,
  bootstrap,
  breath,
  clone,
  morph,
  end,
};
