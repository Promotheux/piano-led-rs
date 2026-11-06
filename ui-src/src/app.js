// Entry of the Svelte build (ui/dist/app.js): mounts the Svelte views and
// exports what the vanilla modules call (main.js, live.js, controls.js, improv.js).
import { mount } from 'svelte';
import Library from './Library.svelte';
import Settings from './Settings.svelte';

export { libraryLive, loadSongs, playerTitle, renderSongs, sortSongs } from './store.svelte.js';
export { loadSettings, savePending, setSetting, settingsHealth } from './settings.svelte.js';

mount(Library, { target: document.querySelector('#library') });
mount(Settings, { target: document.querySelector('#settings') });
