// Apps: improv mode.
import { S } from './state.js';
import { $, $$, post, setSeg, toast } from './util.js';
import { chordName, fullName, KEY_NAMES } from './music.js';
import { setSetting } from '/dist/app.js';

// The Pi recognises (or plays) the chord and lights what fits; the tablet
// names it and holds the controls.
export const improv = (cmd) => post('/api/improv', cmd).catch((e) => toast(e.message, true));
$('#im-key').innerHTML = KEY_NAMES.map((n, i) => `<option value="${i}">${n}</option>`).join('');
export function renderImprov() {
  const m = S.live.improv;
  if (!m) return;
  const backing = m.source === 'backing';
  setSeg('#im-source', m.source);
  setSeg('#im-scale', m.scale);
  setSeg('#im-prog', m.progression);
  $('#im-scale').hidden = backing; // a progression brings its own scale
  $('#im-backing').hidden = !backing;
  if (document.activeElement !== $('#im-key')) $('#im-key').value = m.key;
  if (document.activeElement !== $('#im-tempo')) $('#im-tempo').value = m.tempo;
  $('#im-tempoval').textContent = m.tempo;
  setSeg('#im-lights', m.lights);
  $('#im-start').textContent = m.active ? 'Stop' : 'Start';
  $('#im-chord').textContent = m.active ? chordName(m.chord) || '…' : '\u00a0';
  $('#im-next').textContent = m.active && m.next ? `next: ${chordName(m.next)}` : '';
  $$('#im-beats i').forEach((b, i) => b.classList.toggle('on', m.active && backing && i === m.beat));
  $('#im-help').textContent = backing
    ? 'The piano plays the chords. Improvise with the lit keys.'
    : `Play chords below ${fullName(m.split)}; improvise above it with the lit keys.`;
  if (S.settings) {
    // Legend swatches in the strip's colours.
    $('.sw.chord').style.background = S.settings.left_color;
    $('.sw.scale').style.background = S.settings.right_color;
    $('.sw.scale').style.opacity = 0.45;
  }
}
$('#im-start').addEventListener('click', () => S.live && improv({ active: !S.live.improv.active }));
// Lights and split are settings (kept across restarts): save them as such.
$('#im-lights').addEventListener('click', (e) => {
  if (!e.target.dataset.v || !S.settings) return;
  setSetting('improv_lights', e.target.dataset.v);
});
$('#im-key').addEventListener('change', (e) => improv({ key: Number(e.target.value) }));
$('#im-tempo').addEventListener('input', (e) => ($('#im-tempoval').textContent = e.target.value));
$('#im-tempo').addEventListener('change', (e) => improv({ tempo: Number(e.target.value) }));

for (const [sel, field] of [['#im-source', 'source'], ['#im-scale', 'scale'], ['#im-prog', 'progression']]) {
  $(sel).addEventListener('click', (e) => e.target.dataset.v && improv({ [field]: e.target.dataset.v }));
}
