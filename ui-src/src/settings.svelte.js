// Settings state: the form being edited, the MIDI ports, and the health rows.
// S.settings (state.js) stays the vanilla modules' copy of what the Pi has.
import { S } from '/js/state.js';
import { api, setSeg, toast } from '/js/util.js';

export const cfg = $state({
  form: null, // the settings, plus the form-only fields below
  ports: [], // MIDI input names
  health: {}, // row id -> { text, bad }, from live.js
});

let timer = null;

/** A save is waiting: don't reload settings from the Pi over the edit. */
export const savePending = () => timer !== null;

// The Pi's settings as form fields: the key colour is a mode plus a colour,
// note offsets are edited as "note:LEDs, …", and no MIDI port is ''.
function toForm(s) {
  const single = s.color.startsWith('#');
  return {
    ...s,
    colormode: single ? 'single' : s.color,
    single: single ? s.color : '#ffffff',
    offsets: s.note_offsets.map(([n, o]) => `${n}:${o}`).join(', '),
    midi_port: s.midi_port ?? '',
  };
}

function fromForm(form) {
  const { colormode, single, offsets, ...s } = $state.snapshot(form);
  s.color = colormode === 'single' ? single : colormode;
  s.note_offsets = offsets
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => p.split(':').map(Number));
  s.midi_port = s.midi_port || null;
  return s;
}

export async function loadSettings() {
  try {
    const [s, ports] = await Promise.all([api('/api/settings'), api('/api/ports').catch(() => ({ inputs: [] }))]);
    S.settings = s;
    const names = new Set(ports.inputs.map((p) => p.replace(/\s+\d+:\d+$/, '')));
    if (s.midi_port) names.add(s.midi_port);
    cfg.ports = [...names];
    cfg.form = toForm(s);
    setSeg('#view', s.view_mode);
  } catch (e) {
    toast(e.message, true);
  }
}

/** Save shortly after the last change (sliders sooner, so the LEDs follow). */
export function scheduleSave(delay) {
  clearTimeout(timer);
  timer = setTimeout(save, delay);
}

export async function save() {
  clearTimeout(timer);
  timer = null;
  if (!cfg.form) return;
  try {
    S.settings = await api('/api/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(fromForm(cfg.form)),
    });
    toast('Saved');
  } catch (e) {
    toast(e.message, true);
  }
}

/** Change one setting from elsewhere in the app (player view, improv lights) and save. */
export function setSetting(key, value) {
  if (!cfg.form) return;
  cfg.form[key] = value;
  if (S.settings) S.settings[key] = value; // take effect now; the save syncs other tablets
  save();
}

/** Health rows from live.js: { link, in, out, fps, ws, rtp } -> { text, bad }. */
export function settingsHealth(rows) {
  cfg.health = rows;
}
