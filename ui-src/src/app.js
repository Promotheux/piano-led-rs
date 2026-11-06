// Entry of the Svelte build (ui/dist/app.js): mounts the Svelte views and
// exports what the vanilla modules call (main.js, live.js, controls.js).
import { mount } from 'svelte';
import Library from './Library.svelte';
import Settings from './Settings.svelte';
import Ready from './Ready.svelte';
import Trainer from './Trainer.svelte';
import Improv from './Improv.svelte';
import Harmony from './Harmony.svelte';

export { libraryLive, loadSongs, playerTitle, renderSongs, sortSongs } from './store.svelte.js';
export { loadSettings, savePending, setSetting, settingsHealth } from './settings.svelte.js';
export { appsLive } from './apps.svelte.js';

mount(Library, { target: document.querySelector('#library') });
mount(Settings, { target: document.querySelector('#settings') });
// The Apps keep their "‹ Apps" button in index.html (nav.js wires it on load).
for (const [app, view] of [['trainer', Trainer], ['improv', Improv], ['harmony', Harmony]]) {
  mount(Ready, { target: document.querySelector(`#${app} .app-mount`), props: { app, view } });
}
