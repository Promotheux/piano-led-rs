// Piano LED tablet app. The Pi runs the player; this page draws it and sends commands.
import { draw, resize } from './view.js';
import { connect } from './live.js';
import { loadSettings, loadSongs } from '/dist/app.js';
import './state.js';
import './music.js';
import './metronome.js';
import './nav.js';
import './controls.js';

// Chrome on Android only installs over HTTPS with a service worker.
if ('serviceWorker' in navigator && window.isSecureContext) navigator.serviceWorker.register('/sw.js').catch(() => {});

resize();
requestAnimationFrame(draw);
connect();
loadSongs();
loadSettings();
