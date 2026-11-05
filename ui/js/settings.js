// Settings, Wi-Fi setup and restart/reboot.
import { S } from './state.js';
import { $, api, escapeHtml, post, setSeg, toast } from './util.js';

export const form = $('#settingsform');
export async function loadSettings() {
  try {
    const [s, ports] = await Promise.all([api('/api/settings'), api('/api/ports').catch(() => ({ inputs: [] }))]);
    S.settings = s;
    const portSel = form.midi_port;
    portSel.innerHTML = '<option value="">Automatic</option>';
    const names = new Set(ports.inputs.map((p) => p.replace(/\s+\d+:\d+$/, '')));
    if (s.midi_port) names.add(s.midi_port);
    for (const n of names) portSel.add(new Option(n, n));
    fillForm(s);
  } catch (e) {
    toast(e.message, true);
  }
}
export function fillForm(s) {
  for (const el of form.elements) {
    if (!el.name || !(el.name in s)) continue;
    if (el.type === 'checkbox') el.checked = s[el.name];
    else if (el.name === 'note_offsets') el.value = s.note_offsets.map(([n, o]) => `${n}:${o}`).join(', ');
    else if (el.name === 'color') el.value = s.color.startsWith('#') ? s.color : '#ffffff';
    else el.value = s[el.name] ?? '';
  }
  form.colormode.value = s.color.startsWith('#') ? 'single' : s.color;
  form.color.disabled = form.colormode.value !== 'single';
  setSeg('#view', s.view_mode);
}
export function readForm() {
  const s = { ...S.settings };
  for (const el of form.elements) {
    if (!el.name || !(el.name in s)) continue;
    if (el.type === 'checkbox') s[el.name] = el.checked;
    else if (el.type === 'number' || el.type === 'range' || 'num' in el.dataset) s[el.name] = Number(el.value);
    else if (el.name === 'note_offsets')
      s.note_offsets = el.value.split(',').map((p) => p.trim()).filter(Boolean).map((p) => p.split(':').map(Number));
    else if (el.name === 'midi_port') s.midi_port = el.value || null;
    else s[el.name] = el.value;
  }
  s.color = form.colormode.value === 'single' ? form.color.value : form.colormode.value;
  return s;
}
export let saveTimer;
form.addEventListener('input', (e) => {
  if (!e.target.name) return; // not a setting (e.g. the Wi-Fi fields)
  if (e.target.name === 'colormode') form.color.disabled = form.colormode.value !== 'single';
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, e.target.type === 'range' ? 150 : 600);
});
export async function save() {
  saveTimer = null;
  try {
    S.settings = await api('/api/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(readForm()),
    });
    toast('Saved');
  } catch (e) {
    toast(e.message, true);
  }
}

for (const [id, path, question, done] of [
  ['#restartplv', '/api/system/restart', 'Restart plv? The LEDs and this page pause for a few seconds.', 'Restarting plv…'],
  ['#rebootpi', '/api/system/reboot', 'Reboot the Raspberry Pi? It takes about a minute to come back.', 'Rebooting the Pi… this page reconnects by itself.'],
]) {
  $(id).addEventListener('click', async () => {
    if (!confirm(question)) return;
    try {
      await post(path, {});
      toast(done);
    } catch (e) {
      toast(e.message, true);
    }
  });
}

export async function loadWifiCurrent() {
  try {
    const c = await api('/api/wifi/current');
    $('#wifinow').textContent = !c.ssid ? 'no network' : c.hotspot ? `its own hotspot (${c.ssid})` : c.ssid;
  } catch {
    $('#wifinow').textContent = 'unknown';
  }
}
export async function scanWifi() {
  loadWifiCurrent();
  const list = $('#wifilist');
  list.innerHTML = '<li class="muted">Scanning…</li>';
  try {
    const { networks } = await api('/api/wifi');
    list.innerHTML = networks.length
      ? ''
      : '<li class="muted">No networks found. On its own hotspot the Pi often can\'t scan: type the name below.</li>';
    for (const n of networks) {
      const li = document.createElement('li');
      li.innerHTML = `<span>${escapeHtml(n.ssid)}${n.active ? ' ✓' : ''}</span><span class="meta">${n.signal}%${n.secure ? ' 🔒' : ''}</span>`;
      li.addEventListener('click', () => {
        $('#wifissid').value = n.ssid;
        $('#wifipass').focus();
      });
      list.append(li);
    }
  } catch (e) {
    list.innerHTML = '';
    toast(e.message, true);
  }
}
$('#wifiscan').addEventListener('click', scanWifi);
loadWifiCurrent();
$('#wificonnect').addEventListener('click', async () => {
  const ssid = $('#wifissid').value.trim();
  const password = $('#wifipass').value;
  if (!ssid) return toast('Pick or type a network name', true);
  if (!confirm(`Switch the Pi to "${ssid}"? This page loses its connection while the Pi switches.`)) return;
  try {
    await post('/api/wifi', { ssid, password });
    $('#wifipass').value = '';
    $('#wifistatus').textContent =
      `Switching to "${ssid}". Join that network with this tablet too, then find the Pi in your router ` +
      `(it's called pianoledvisualizer) or try http://pianoledvisualizer.local/. ` +
      `If the password was wrong, the PianoLED-Setup hotspot comes back within a minute.`;
  } catch (e) {
    toast(e.message, true);
  }
});
