// Piano LED tablet app. The Pi runs the player; this page draws it and sends commands.
import { $ } from './util.js';
import { draw, resize } from './view.js';
import { connect } from './live.js';
import { loadSongs } from '/dist/library.js';
import { loadSettings } from './settings.js';
import './state.js';
import './music.js';
import './metronome.js';
import './nav.js';
import './controls.js';
import './trainer.js';
import './improv.js';
import './harmony.js';

// Chrome on Android only installs over HTTPS with a service worker.
if ('serviceWorker' in navigator && window.isSecureContext) navigator.serviceWorker.register('/sw.js').catch(() => {});
$('#httpslink').href = `https://${location.hostname}${location.port === '8080' ? ':8443' : ''}/`;
$('#httpslink').textContent = $('#httpslink').href;
$('#installstate').textContent = window.isSecureContext
  ? 'This page is secure, so Chrome can install it (menu → Install app).'
  : 'This page is plain http, so Chrome can only make a shortcut. Follow the steps below.';

resize();
requestAnimationFrame(draw);
connect();
loadSongs();
loadSettings();
