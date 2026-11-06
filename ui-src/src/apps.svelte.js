// Live state of the Apps (pitch trainer, improv, chord progressions), as the
// Pi pushes it. live.js hands every update over with appsLive().
import { post, toast } from '/js/util.js';

export const apps = $state({ trainer: null, improv: null, harmony: null });

export function appsLive(live) {
  apps.trainer = live.trainer;
  apps.improv = live.improv;
  apps.harmony = live.harmony;
}

/** A command poster for one app's API, e.g. command('/api/trainer')({ active: true }). */
export const command = (path) => (cmd) => post(path, cmd).catch((e) => toast(e.message, true));
