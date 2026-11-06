// The live connection to the Pi: state 20× a second over a WebSocket.
import { S } from './state.js';
import { $, api, ICON, setSeg, toast } from './util.js';
import { prepareSpelling } from './music.js';
import { renderTracks, showScorecard } from './controls.js';
import { libraryLive, playerTitle } from '/dist/library.js';
import { loadSettings, saveTimer } from './settings.js';
import { renderTrainer } from './trainer.js';
import { renderImprov } from './improv.js';
import { renderHarmony } from './harmony.js';

export function connect() {
  const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`);
  ws.onopen = () => {
    wsOpen = true;
    renderHealth();
  };
  ws.onclose = () => {
    wsOpen = false;
    renderHealth();
    setTimeout(connect, 1500);
  };
  ws.onmessage = (e) => {
    S.live = JSON.parse(e.data);
    S.liveAt = performance.now();
    onLive();
  };
}

// Health: the Pi pushes 20 times a second, so 2 s of silence means the link
// (or the Pi) froze even if the socket still looks open.
export let wsOpen = false;
export function renderHealth() {
  const age = S.live ? (performance.now() - S.liveAt) / 1000 : Infinity;
  const stale = !wsOpen || age > 2;
  const h = stale ? null : S.live.health;
  const set = (id, text, bad) => {
    $(id).textContent = text;
    $(id).classList.toggle('bad', !!bad);
  };
  set('#h-link', !wsOpen ? 'Reconnecting…' : stale ? `No data for ${Math.round(age)} s` : 'Live', stale);
  // A network session carries MIDI both ways, so it counts as an input and an output.
  const net = h && h.rtp_peers && h.rtp_peers.length ? `network: ${h.rtp_peers.join(', ')}` : '';
  const ins = h ? [h.midi_in, net].filter(Boolean).join(' + ') : '';
  set('#h-in', h ? ins || 'No keyboard found' : '–', h && !ins);
  const outs = h ? [h.midi_out, net].filter(Boolean).join(' + ') : '';
  set('#h-out', h ? outs || 'None' : '–', h && !outs);
  set('#h-fps', h ? `${h.led_fps} fps` : '–', h && !h.led_fps);
  set('#h-ws', h ? String(h.ws_clients) : '–');
  set('#h-rtp', h ? (h.rtp_peers && h.rtp_peers.length ? h.rtp_peers.join(', ') : 'No computer connected') : '–');
  const problem = stale ? 'No connection to the Pi' : !h.midi_in ? 'No keyboard connected' : !h.led_fps ? 'LEDs stalled' : '';
  $('#conn').classList.toggle('on', !problem);
  $('#conn').title = problem || 'Connected';
}
setInterval(renderHealth, 1000);

export let uiVersion = null; // the Pi's UI build this page came from
export function onLive() {
  // The Pi runs a newer UI: reload, so this page can't save settings it doesn't know.
  if (uiVersion !== null && S.live.ui !== uiVersion) return location.reload();
  uiVersion = S.live.ui;
  renderTrainer();
  renderImprov();
  renderHarmony();
  // Another tablet (or this one) changed settings: reload them.
  if (S.settingsGen !== null && S.live.settings_gen !== S.settingsGen && !saveTimer) loadSettings();
  S.settingsGen = S.live.settings_gen;
  if (S.live.song && (!S.song || S.song.name !== S.live.song)) loadSong();
  if (!S.live.song) S.song = null;
  if ($('#play').dataset.playing !== String(S.live.playing)) {
    $('#play').dataset.playing = S.live.playing; // swap the icon only when it changes
    $('#play').innerHTML = S.live.playing ? ICON.pause : ICON.play;
  }
  $('#play').setAttribute('aria-label', S.live.playing ? 'Pause' : 'Play');
  $('#waiting').hidden = !(S.live.playing && S.live.waiting);
  $('#score').textContent = S.live.mode !== 'listen' && S.live.right + S.live.wrong ? `✓ ${S.live.right}  ✗ ${S.live.wrong}` : '';
  showScorecard(S.live.result);
  $('#empty').hidden = !!S.live.song;
  setSeg('#mode', S.live.mode);
  setSeg('#hand', S.live.hand);
  if (document.activeElement !== $('#tempo')) {
    $('#tempo').value = S.live.tempo;
    $('#tempoval').textContent = S.live.tempo + '%';
  }
  const scrub = $('#scrub');
  scrub.max = Math.max(1, S.live.len);
  if (!S.scrubbing) scrub.value = Math.max(0, S.live.pos);
  $('#loop [data-v=a]').classList.toggle('on', S.loopA !== null || !!S.live.loop_ab);
  $('#loop [data-v=b]').classList.toggle('on', !!S.live.loop_ab);
  $('#ramp').disabled = !S.live.loop_ab;
  $('#countin').classList.toggle('on', S.live.count_in);
  $('#countin').setAttribute('aria-pressed', S.live.count_in);
  $('#ramp').classList.toggle('on', S.live.ramp);
  $('#ramp').setAttribute('aria-pressed', S.live.ramp);
  $('#ramp').textContent = S.live.ramp && S.live.loop_ab && S.live.mistakes ? `Speed up · ${S.live.mistakes} ✗` : 'Speed up';
  libraryLive(S.live); // the Svelte Library: loaded song and recording state
}

export async function loadSong() {
  const name = S.live.song;
  try {
    const s = await api('/api/player/song');
    if (s.name !== name) return;
    prepareSpelling(s);
    S.song = s;
    S.maxDur = s.notes.reduce((m, n) => Math.max(m, n[1]), 0);
    $('#title').textContent = playerTitle(s.name);
    renderTracks();
  } catch (e) {
    toast(e.message, true);
  }
}
