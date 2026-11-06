// Entry of the Svelte build (ui/dist/library.js): mounts the Library view and
// exports what the vanilla modules call (main.js, live.js, controls.js).
import { mount } from 'svelte';
import Library from './Library.svelte';

export { libraryLive, loadSongs, playerTitle, renderSongs, sortSongs } from './store.svelte.js';

mount(Library, { target: document.querySelector('#library') });
