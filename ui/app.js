'use strict';
// Piano LED tablet app. The Pi runs the player; this page draws it and sends commands.

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const LOOKAHEAD_MS = 3000;
const FIRST = 21, LAST = 108; // A0..C8

let live = null;        // last state pushed by the Pi
let liveAt = 0;         // performance.now() when it arrived
let song = null;        // { name, len, tracks, notes: [[t, dur, key, hand, far]] }
let maxDur = 0;
let settings = null;
let songs = [];
let scrubbing = false;
let loopA = null;
let settingsGen = null; // last settings generation seen from the Pi

// ---------- helpers ----------
async function api(path, opts = {}) {
  const res = await fetch(path, opts);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || res.statusText);
  return body;
}
const post = (path, data) =>
  api(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
const player = (cmd) => post('/api/player', cmd).catch((e) => toast(e.message, true));

let toastTimer;
function toast(msg, error = false) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = 'show' + (error ? ' error' : '');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.className = ''), 2500);
}
const fmt = (ms) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
};
const prettyName = (n) => n.replace(/\.midi?$/i, '').replace(/[_]+/g, ' ');

// ---------- navigation ----------
function show(view) {
  $$('.tab').forEach((b) => b.classList.toggle('active', b.dataset.view === view));
  $$('.view').forEach((v) => v.classList.toggle('active', v.id === view));
  if (view === 'player') resize();
}
$$('.tab').forEach((b) => b.addEventListener('click', () => show(b.dataset.view)));
$('#fullscreen').addEventListener('click', () => {
  if (document.fullscreenElement) document.exitFullscreen();
  else document.documentElement.requestFullscreen?.().catch(() => {});
});

// ---------- live connection ----------
function connect() {
  const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`);
  ws.onopen = () => $('#conn').classList.add('on');
  ws.onclose = () => {
    $('#conn').classList.remove('on');
    setTimeout(connect, 1500);
  };
  ws.onmessage = (e) => {
    live = JSON.parse(e.data);
    liveAt = performance.now();
    onLive();
  };
}

function onLive() {
  // Another tablet (or this one) changed settings: reload them.
  if (settingsGen !== null && live.settings_gen !== settingsGen && !saveTimer) loadSettings();
  settingsGen = live.settings_gen;
  if (live.song && (!song || song.name !== live.song)) loadSong();
  if (!live.song) song = null;
  $('#play').textContent = live.playing ? '⏸' : '▶';
  $('#play').setAttribute('aria-label', live.playing ? 'Pause' : 'Play');
  $('#waiting').hidden = !(live.playing && live.waiting);
  $('#empty').hidden = !!live.song;
  setSeg('#mode', live.mode);
  setSeg('#hand', live.hand);
  if (document.activeElement !== $('#tempo')) {
    $('#tempo').value = live.tempo;
    $('#tempoval').textContent = live.tempo + '%';
  }
  const scrub = $('#scrub');
  scrub.max = Math.max(1, live.len);
  if (!scrubbing) scrub.value = Math.max(0, live.pos);
  $('#loop [data-v=a]').classList.toggle('on', loopA !== null || !!live.loop_ab);
  $('#loop [data-v=b]').classList.toggle('on', !!live.loop_ab);
  $$('#songs li').forEach((li) => li.classList.toggle('current', li.dataset.name === live.song));
}

async function loadSong() {
  const name = live.song;
  try {
    const s = await api('/api/player/song');
    if (s.name !== name) return;
    song = s;
    maxDur = s.notes.reduce((m, n) => Math.max(m, n[1]), 0);
    $('#title').textContent = prettyName(s.name);
    renderTracks();
  } catch (e) {
    toast(e.message, true);
  }
}

function setSeg(sel, value) {
  $$(`${sel} button`).forEach((b) => b.classList.toggle('on', b.dataset.v === value));
}

// ---------- controls ----------
$('#play').addEventListener('click', () => live && player({ playing: !live.playing }));
$('#restart').addEventListener('click', () => player({ seek: -2000 }));
$('#scrub').addEventListener('input', () => (scrubbing = true));
$('#scrub').addEventListener('change', (e) => {
  scrubbing = false;
  player({ seek: Number(e.target.value) });
});
$('#tempo').addEventListener('input', (e) => ($('#tempoval').textContent = e.target.value + '%'));
$('#tempo').addEventListener('change', (e) => player({ tempo: Number(e.target.value) }));
$$('#mode button').forEach((b) => b.addEventListener('click', () => player({ mode: b.dataset.v })));
$$('#hand button').forEach((b) => b.addEventListener('click', () => player({ hand: b.dataset.v })));
$$('#view button').forEach((b) =>
  b.addEventListener('click', () => {
    if (!settings) return;
    form.view_mode.value = b.dataset.v;
    settings.view_mode = b.dataset.v; // switch now; the save syncs other tablets
    setSeg('#view', b.dataset.v);
    save();
  })
);
$$('#loop button').forEach((b) =>
  b.addEventListener('click', () => {
    const pos = displayPos();
    if (b.dataset.v === 'a') {
      loopA = pos;
      toast('Loop start set, now tap B at the end');
    } else if (b.dataset.v === 'b') {
      if (loopA === null || pos <= loopA) return toast('Set A first, then B later in the song', true);
      player({ loop_ab: [loopA, pos], seek: loopA });
      loopA = null;
    } else {
      loopA = null;
      player({ loop_ab: [] });
    }
  })
);

// Tracks dialog: which hand plays each track.
$('#tracksbtn').addEventListener('click', () => song && $('#tracks').showModal());
function renderTracks() {
  const list = $('#tracklist');
  list.innerHTML = '';
  song.tracks.forEach((t, i) => {
    if (!t.notes) return;
    const row = document.createElement('div');
    row.className = 'trackrow';
    row.innerHTML = `<span>${escapeHtml(t.name || 'Track ' + (i + 1))} <small>${t.notes} notes</small></span>`;
    const sel = document.createElement('select');
    sel.innerHTML = '<option value="right">Right hand</option><option value="left">Left hand</option>' +
      '<option value="split">Both hands (split at middle C)</option><option value="ignore">Ignore</option>';
    sel.value = t.role;
    sel.addEventListener('change', async () => {
      // At most one track per hand: anything this choice overlaps becomes ignored.
      const v = sel.value;
      const uses = (r) => (r === 'split' ? ['right', 'left'] : r === 'ignore' ? [] : [r]);
      const roles = song.tracks.map((tr, j) =>
        j === i ? v : uses(tr.role).some((h) => uses(v).includes(h)) ? 'ignore' : tr.role
      );
      await player({ track_roles: roles });
      song = null; // refetch with the new roles
    });
    row.append(sel);
    list.append(row);
  });
}
const escapeHtml = (s) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

// ---------- library ----------
async function loadSongs() {
  try {
    songs = (await api('/api/songs')).songs;
    renderSongs();
  } catch (e) {
    toast(e.message, true);
  }
}
function renderSongs() {
  const q = $('#search').value.trim().toLowerCase();
  const ul = $('#songs');
  ul.innerHTML = '';
  for (const s of songs) {
    if (q && !s.name.toLowerCase().includes(q)) continue;
    const li = document.createElement('li');
    li.dataset.name = s.name;
    li.classList.toggle('current', live && live.song === s.name);
    li.innerHTML = `<span class="name">${escapeHtml(prettyName(s.name))}</span><span class="size">${Math.max(1, Math.round(s.size / 1024))} KB</span>`;
    li.addEventListener('click', async () => {
      await player({ song: s.name });
      show('player');
    });
    ul.append(li);
  }
}
$('#search').addEventListener('input', renderSongs);
$('#upload').addEventListener('change', async (e) => {
  for (const f of e.target.files) {
    try {
      await api(`/api/songs?name=${encodeURIComponent(f.name)}`, { method: 'POST', body: f });
      toast(`Added ${prettyName(f.name)}`);
    } catch (err) {
      toast(`${f.name}: ${err.message}`, true);
    }
  }
  e.target.value = '';
  loadSongs();
});

// ---------- settings ----------
const form = $('#settingsform');
async function loadSettings() {
  try {
    const [s, ports] = await Promise.all([api('/api/settings'), api('/api/ports').catch(() => ({ inputs: [] }))]);
    settings = s;
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
function fillForm(s) {
  for (const el of form.elements) {
    if (!el.name || !(el.name in s)) continue;
    if (el.type === 'checkbox') el.checked = s[el.name];
    else if (el.name === 'note_offsets') el.value = s.note_offsets.map(([n, o]) => `${n}:${o}`).join(', ');
    else if (el.name === 'color') el.value = s.color === 'rainbow' ? '#ffffff' : s.color;
    else el.value = s[el.name] ?? '';
  }
  form.colormode.value = s.color === 'rainbow' ? 'rainbow' : 'single';
  form.color.disabled = s.color === 'rainbow';
  setSeg('#view', s.view_mode);
}
function readForm() {
  const s = { ...settings };
  for (const el of form.elements) {
    if (!el.name || !(el.name in s)) continue;
    if (el.type === 'checkbox') s[el.name] = el.checked;
    else if (el.type === 'number' || el.type === 'range') s[el.name] = Number(el.value);
    else if (el.name === 'note_offsets')
      s.note_offsets = el.value.split(',').map((p) => p.trim()).filter(Boolean).map((p) => p.split(':').map(Number));
    else if (el.name === 'midi_port') s.midi_port = el.value || null;
    else s[el.name] = el.value;
  }
  s.color = form.colormode.value === 'rainbow' ? 'rainbow' : form.color.value;
  return s;
}
let saveTimer;
form.addEventListener('input', (e) => {
  if (e.target.name === 'colormode') form.color.disabled = form.colormode.value === 'rainbow';
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, e.target.type === 'range' ? 150 : 600);
});
async function save() {
  saveTimer = null;
  try {
    settings = await api('/api/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(readForm()),
    });
    toast('Saved');
  } catch (e) {
    toast(e.message, true);
  }
}

// ---------- falling notes ----------
const canvas = $('#roll');
const ctx = canvas.getContext('2d');
let W = 0, H = 0, keyTop = 0, keys = [];

function resize() {
  const r = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  W = r.width;
  H = r.height;
  canvas.width = Math.round(W * dpr);
  canvas.height = Math.round(H * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  keyTop = H - Math.min(140, Math.max(70, H * 0.22));
  // Key geometry: 52 white keys across the width, black keys on the boundaries.
  const whiteW = W / 52;
  keys = [];
  let white = 0;
  for (let n = FIRST; n <= LAST; n++) {
    const black = [1, 3, 6, 8, 10].includes(n % 12);
    keys[n] = black
      ? { x: white * whiteW - whiteW * 0.3, w: whiteW * 0.6, black }
      : { x: white++ * whiteW, w: whiteW, black };
  }
}
window.addEventListener('resize', resize);

function displayPos() {
  if (!live) return 0;
  const run = live.playing && !live.waiting ? (performance.now() - liveAt) * (live.tempo / 100) : 0;
  return live.pos + Math.min(run, 250); // never extrapolate far past the last update
}

function practised(n) {
  if (!live || live.mode === 'listen' || n[4]) return false;
  return live.hand === 'both' || (live.hand === 'right') === (n[3] === 0);
}

function handColor(hand) {
  if (!settings) return hand ? '#ff8a1f' : '#2f7cff';
  return hand ? settings.left_color : settings.right_color;
}

// Index of the first note starting at or after `t` (notes are sorted by start).
function firstFrom(t) {
  const notes = song.notes;
  let lo = 0, hi = notes.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (notes[mid][0] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function draw() {
  requestAnimationFrame(draw);
  if (!W || !$('#player').classList.contains('active')) return;
  ctx.clearRect(0, 0, W, H);
  const pos = displayPos();

  const active = new Map(); // key -> hand of the song note sounding now
  const expected = new Set(); // keys you should be playing now (same window as the Pi)
  if (song) {
    const notes = song.notes;
    for (let i = firstFrom(pos - maxDur); i < notes.length; i++) {
      const n = notes[i];
      const [t, dur, key, hand] = n;
      if (t > pos + 300) break;
      if (t + dur < pos) continue;
      // While waiting, light the chord you're expected to play.
      const ahead = live && live.waiting && practised(n) ? 61 : 0;
      if (t <= pos + ahead) active.set(key, hand);
      if (practised(n) && t <= pos + (live.waiting ? 61 : 250)) expected.add(key);
    }
    if (settings && settings.view_mode === 'sheet') drawSheet(pos);
    else drawRoll(pos);
  }
  drawKeyboard(active, expected);
  if (live) $('#time').textContent = `${fmt(pos)} / ${fmt(live.len)}`;
}

// Waterfall: notes fall onto the keyboard.
function drawRoll(pos) {
  const pxPerMs = keyTop / LOOKAHEAD_MS;
  const names = settings && settings.note_names;
  // Octave guide lines at every C.
  ctx.fillStyle = '#161b26';
  for (let n = 24; n <= LAST; n += 12) ctx.fillRect(keys[n].x, 0, 1, keyTop);
  const notes = song.notes;
  for (let i = firstFrom(pos - maxDur); i < notes.length; i++) {
    const n = notes[i];
    const [t, dur, key, hand] = n;
    if (t > pos + LOOKAHEAD_MS) break;
    if (t + dur < pos || !keys[key]) continue;
    const k = keys[key];
    const y1 = keyTop - (t - pos) * pxPerMs;
    const y0 = keyTop - (t + dur - pos) * pxPerMs;
    ctx.globalAlpha = practised(n) ? 1 : 0.35;
    ctx.fillStyle = handColor(hand);
    roundRect(k.x + 1, Math.max(0, y0), k.w - 2, Math.min(keyTop, y1) - Math.max(0, y0), 4);
    if (n[4]) { // out of reach: the piano plays it
      ctx.globalAlpha = 0.8;
      ctx.strokeStyle = handColor(hand);
      ctx.setLineDash([4, 3]);
      ctx.strokeRect(k.x + 1.5, Math.max(0, y0) + 0.5, k.w - 3, Math.min(keyTop, y1) - Math.max(0, y0) - 1);
      ctx.setLineDash([]);
    }
    // Name at the bottom of the note, like Synthesia, when there's room.
    if (names && Math.min(keyTop, y1) - Math.max(0, y0) > 14) {
      const label = noteName(key);
      const bottom = Math.min(keyTop, y1) - 3;
      ctx.globalAlpha = 1;
      ctx.font = `700 ${fitFont(label, k.w - 3, 14)}px system-ui, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'bottom';
      ctx.lineWidth = 3;
      ctx.strokeStyle = 'rgba(0,0,0,.55)';
      ctx.strokeText(label, k.x + k.w / 2, bottom);
      ctx.fillStyle = '#ffffff';
      ctx.fillText(label, k.x + k.w / 2, bottom);
      ctx.lineWidth = 1;
    }
  }
  ctx.globalAlpha = 1;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
}

// ---------- sheet music ----------
// Spelling per pitch class: [letter, letter index C=0..B=6, accidental].
const SPELL = [['C', 0, ''], ['C', 0, '♯'], ['D', 1, ''], ['E', 2, '♭'], ['E', 2, ''], ['F', 3, ''],
  ['F', 3, '♯'], ['G', 4, ''], ['A', 5, '♭'], ['A', 5, ''], ['B', 6, '♭'], ['B', 6, '']];
// Letter name of `key`, with or without its sharp/flat.
const noteName = (key, withAcc = true) => SPELL[key % 12][0] + (withAcc ? SPELL[key % 12][2] : '');
// Font size (px, bold) at which `text` fits in `width`, capped at `max`.
function fitFont(text, width, max) {
  ctx.font = `700 ${max}px system-ui, sans-serif`;
  const w = ctx.measureText(text).width;
  return w <= width ? max : Math.max(7, (max * width) / w);
}
// Diatonic step number (C4 = 28): one step per staff line or space.
const diatonic = (key) => (Math.floor(key / 12) - 1) * 7 + SPELL[key % 12][1];
const TREBLE_BOTTOM = 30; // E4
const BASS_TOP = 26; // A3
const SHEET_AHEAD_MS = 4000;
const CHORD_MS = 60; // same chord window as the Pi's wait mode

// Grand staff scrolling right to left past a playhead; right hand on the
// treble staff, left hand on the bass staff.
function drawSheet(pos) {
  const g = Math.max(5, Math.min(24, keyTop / 22)); // gap between staff lines
  const mid = keyTop / 2;
  const trebleY = (d) => mid - 2 * g - (d - TREBLE_BOTTOM) * (g / 2);
  const bassY = (d) => mid + 2 * g - (d - BASS_TOP) * (g / 2);
  const clefW = 4 * g;
  const playX = Math.max(clefW + 2 * g, W * 0.22);
  const pxPerMs = (W - playX) / SHEET_AHEAD_MS;
  const behindMs = (playX - clefW) / pxPerMs;

  // Staves and clefs.
  ctx.strokeStyle = '#59627a';
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 0; i < 5; i++) {
    for (const y of [trebleY(TREBLE_BOTTOM + 2 * i), bassY(BASS_TOP - 2 * i)]) {
      ctx.moveTo(g * 0.5, Math.round(y) + 0.5);
      ctx.lineTo(W, Math.round(y) + 0.5);
    }
  }
  ctx.moveTo(g * 0.5 + 0.5, trebleY(38));
  ctx.lineTo(g * 0.5 + 0.5, bassY(18));
  ctx.stroke();
  ctx.fillStyle = '#c3c9d8';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  ctx.font = `${6.2 * g}px "Noto Music", "Bravura", "Apple Symbols", "Segoe UI Symbol", serif`;
  ctx.fillText('𝄞', g * 0.8, trebleY(TREBLE_BOTTOM) + 1.3 * g);
  ctx.font = `${3.6 * g}px "Noto Music", "Bravura", "Apple Symbols", "Segoe UI Symbol", serif`;
  ctx.fillText('𝄢', g * 0.9, bassY(BASS_TOP) + 2.9 * g);

  // Playhead.
  ctx.fillStyle = 'rgba(109,139,255,.9)';
  ctx.fillRect(playX - 1, trebleY(42), 2, bassY(14) - trebleY(42));

  const notes = song.notes;
  const names = settings && settings.note_names;
  ctx.save();
  ctx.beginPath();
  ctx.rect(clefW, 0, W - clefW, keyTop);
  ctx.clip();

  // Bar lines (computed on the Pi from the file's time signature).
  ctx.fillStyle = '#59627a';
  for (const b of song.bars || []) {
    const bx = playX + (b - pos) * pxPerMs;
    if (bx < clefW || bx > W) continue;
    ctx.fillRect(Math.round(bx), trebleY(38), 1.5, bassY(18) - trebleY(38));
  }

  // Head size: larger when the note name is printed inside.
  const rx = g * (names ? 0.95 : 0.66);
  const ry = g * (names ? 0.66 : 0.46);
  // Group visible notes into chords: same staff, starting within CHORD_MS of
  // the chord's first note (the same window the Pi uses for wait mode).
  const chords = [];
  const open = [null, null]; // current chord per staff: 0 treble, 1 bass
  for (let i = firstFrom(pos - behindMs - maxDur); i < notes.length; i++) {
    const n = notes[i];
    const [t, dur, , hand] = n;
    if (t > pos + SHEET_AHEAD_MS) break;
    if (t + dur < pos - behindMs) continue;
    const staff = hand === 0 ? 0 : 1;
    let c = open[staff];
    if (!c || t - c.t > CHORD_MS) {
      c = open[staff] = { t, staff, notes: [] };
      chords.push(c);
    }
    c.notes.push(n);
  }
  for (const c of chords) drawChord(c);

  function drawChord(c) {
    const treble = c.staff === 0;
    const Y = (d) => (treble ? trebleY(d) : bassY(d));
    const midLine = treble ? TREBLE_BOTTOM + 4 : BASS_TOP - 4; // B4 / D3
    const x = playX + (c.t - pos) * pxPerMs;
    const color = handColor(treble ? 0 : 1);
    const mine = c.notes.some(practised);
    // Heads from bottom to top, with their staff step.
    const heads = c.notes.map((n) => ({ n, d: diatonic(n[2]) })).sort((a, b) => a.d - b.d);
    const bottom = heads[0].d;
    const top = heads[heads.length - 1].d;
    // The note furthest from the middle line decides: above -> stem down (left),
    // below -> stem up (right). Equal distance -> down.
    const up = midLine - bottom > top - midLine;
    const stemX = up ? x + rx * 0.92 : x - rx * 0.92;
    // Seconds: walking from the head where the stem starts towards its tip, a
    // head one step from an unshifted neighbour moves to the other side of the stem.
    const order = up ? heads : [...heads].reverse();
    let prev = null;
    for (const h of order) {
      h.x = x;
      if (prev && Math.abs(h.d - prev.d) === 1 && prev.x === x) h.x = x + (up ? 2 : -2) * rx * 0.92;
      prev = h;
    }

    // Ledger lines, wide enough for shifted heads.
    ctx.globalAlpha = 1;
    ctx.strokeStyle = '#59627a';
    ctx.lineWidth = 1;
    ctx.beginPath();
    const minX = Math.min(...heads.map((h) => h.x)) - rx * 1.6;
    const maxX = Math.max(...heads.map((h) => h.x)) + rx * 1.6;
    const ledger = (from, to, step) => {
      for (let l = from; step < 0 ? l >= to : l <= to; l += step) {
        const ly = Math.round(Y(l)) + 0.5;
        ctx.moveTo(minX, ly);
        ctx.lineTo(maxX, ly);
      }
    };
    if (treble) {
      if (bottom <= TREBLE_BOTTOM - 2) ledger(TREBLE_BOTTOM - 2, bottom, -2);
      if (top >= TREBLE_BOTTOM + 10) ledger(TREBLE_BOTTOM + 10, top, 2);
    } else {
      if (top >= BASS_TOP + 2) ledger(BASS_TOP + 2, top, 2);
      if (bottom <= BASS_TOP - 10) ledger(BASS_TOP - 10, bottom, -2);
    }
    ctx.stroke();

    // One shared stem from the far head through to beyond the stem-end head.
    // Value: 0 whole, 1 half, 2 quarter, 3 eighth, 4 sixteenth (+ dot); the
    // shortest note sets the flags, and an all-whole chord has no stem.
    const kind = Math.max(...c.notes.map((n) => (n[5] ?? 4) >> 1));
    ctx.globalAlpha = mine ? 1 : 0.45;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    if (kind >= 1) {
      const from = up ? Y(bottom) : Y(top);
      const tip = up ? Y(top) - 3.4 * g : Y(bottom) + 3.4 * g;
      ctx.lineWidth = Math.max(1.2, g * 0.12);
      ctx.beginPath();
      ctx.moveTo(stemX, from);
      ctx.lineTo(stemX, tip);
      for (let f = 0; f < kind - 2; f++) {
        const fy = tip + (up ? 1 : -1) * f * 0.8 * g;
        ctx.moveTo(stemX, fy);
        ctx.quadraticCurveTo(stemX + 1.1 * g, fy + (up ? 1 : -1) * 0.9 * g, stemX + 0.7 * g, fy + (up ? 1 : -1) * 2 * g);
      }
      ctx.stroke();
    }

    // Heads, dots and names.
    for (const { n, d, x: hx } of heads) {
      const [t, dur, key] = n;
      const value = n[5] ?? 4;
      const hollow = value >> 1 <= 1;
      const y = Y(d);
      ctx.globalAlpha = practised(n) ? 1 : 0.45;
      ctx.strokeStyle = color;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.ellipse(hx, y, rx, ry, -0.35, 0, Math.PI * 2);
      if (hollow) {
        ctx.lineWidth = Math.max(1.5, g * 0.2);
        ctx.stroke();
      } else {
        ctx.fill();
      }
      if (n[4]) { // out of reach: the piano plays it
        ctx.globalAlpha = 0.9;
        ctx.setLineDash([3, 2]);
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.setLineDash([]);
      }
      if (t <= pos && pos < t + dur) { // sounding now
        ctx.globalAlpha = 1;
        ctx.strokeStyle = '#ffffff';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.ellipse(hx, y, rx + 1.5, ry + 1.5, -0.35, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.lineWidth = 1;
      ctx.globalAlpha = practised(n) ? 1 : 0.45;
      ctx.fillStyle = color;
      if (value & 1) { // dot, nudged into the space when the head sits on a line
        ctx.beginPath();
        ctx.arc(Math.max(...heads.map((h) => h.x)) + rx + 0.5 * g, d % 2 === 0 ? y - g * 0.3 : y, g * 0.16, 0, Math.PI * 2);
        ctx.fill();
      }
      if (names) {
        const letter = noteName(key, false);
        ctx.fillStyle = hollow ? color : '#ffffff';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.font = `700 ${fitFont(letter, rx * 1.75, Math.max(8, ry * 1.45))}px system-ui, sans-serif`;
        ctx.fillText(letter, hx, y + 0.5);
      }
    }

    // Sharps/flats left of the chord, top to bottom; one that would collide
    // with the previous (within a sixth) moves one column further left.
    const accX = minX + rx * 1.6 - rx - 0.15 * g;
    let lastD = null;
    let col = 0;
    ctx.globalAlpha = mine ? 1 : 0.6;
    ctx.fillStyle = '#e8ebf3';
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    ctx.font = `${1.5 * g}px serif`;
    for (const { n, d } of [...heads].reverse()) {
      const acc = SPELL[n[2] % 12][2];
      if (!acc) continue;
      col = lastD !== null && lastD - d < 6 ? col + 1 : 0;
      ctx.fillText(acc, accX - col * 1.1 * g, Y(d));
      lastD = d;
    }
  }
  ctx.restore();
  ctx.globalAlpha = 1;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
}

// Keyboard: whites, then blacks on top.
function drawKeyboard(active, expected) {
  const held = new Set(live ? live.held : []);
  const names = settings && settings.note_names;
  const practising = live && live.playing && song && live.mode !== 'listen';
  const keyH = H - keyTop;
  for (const pass of [false, true]) {
    for (let n = FIRST; n <= LAST; n++) {
      const k = keys[n];
      if (k.black !== pass) continue;
      const h = pass ? keyH * 0.62 : keyH;
      let fill = pass ? '#11141b' : '#e9ecf3';
      if (active.has(n)) fill = shade(handColor(active.get(n)), pass ? 0.75 : 0.55);
      if (held.has(n)) fill = !practising || expected.has(n) ? '#3ddc97' : '#ff5a5a';
      ctx.fillStyle = fill;
      ctx.fillRect(k.x + (pass ? 0 : 0.5), keyTop, k.w - (pass ? 0 : 1), h);
      // Note names on the keys to play now.
      if (names && active.has(n)) {
        ctx.fillStyle = pass ? '#ffffff' : '#11141b';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        const label = noteName(n);
        ctx.font = `700 ${fitFont(label, k.w - 2, 14)}px system-ui, sans-serif`;
        ctx.fillText(label, k.x + k.w / 2, keyTop + h - 4);
      }
    }
  }
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  ctx.fillStyle = '#6d8bff';
  ctx.fillRect(0, keyTop - 2, W, 2);
}

function roundRect(x, y, w, h, r) {
  if (h <= 0) return;
  r = Math.min(r, h / 2, w / 2);
  ctx.beginPath();
  ctx.roundRect ? ctx.roundRect(x, y, w, h, r) : ctx.rect(x, y, w, h);
  ctx.fill();
}

// Mix a #rrggbb colour towards white (amount 0..1 keeps that much colour).
function shade(hex, amount) {
  const v = parseInt(hex.slice(1), 16);
  const mix = (c) => Math.round(c * amount + 255 * (1 - amount));
  return `rgb(${mix(v >> 16)},${mix((v >> 8) & 255)},${mix(v & 255)})`;
}

// ---------- install as app ----------
// Chrome on Android only installs over HTTPS with a service worker.
if ('serviceWorker' in navigator && window.isSecureContext) navigator.serviceWorker.register('/sw.js').catch(() => {});
$('#httpslink').href = `https://${location.hostname}${location.port === '8080' ? ':8443' : ''}/`;
$('#httpslink').textContent = $('#httpslink').href;
$('#installstate').textContent = window.isSecureContext
  ? 'This page is secure, so Chrome can install it (menu → Install app).'
  : 'This page is plain http, so Chrome can only make a shortcut. Follow the steps below.';

// ---------- start ----------
resize();
requestAnimationFrame(draw);
connect();
loadSongs();
loadSettings();
