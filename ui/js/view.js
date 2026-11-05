// The player's canvas: falling notes, sheet music and the keyboard.
import { FIRST, LAST, LOOKAHEAD_MS, S } from './state.js';
import { $, fmt } from './util.js';
import { diatonic, keyAt, noteName, SPELL } from './music.js';
import { tickMetronome } from './metronome.js';

export const canvas = $('#roll');
export const ctx = canvas.getContext('2d');
export let W = 0, H = 0, keyTop = 0, keys = [];

export function resize() {
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

export let shownPos = 0;
export function displayPos() {
  if (!S.live) return 0;
  const run = S.live.playing && !S.live.waiting ? (performance.now() - S.liveAt) * (S.live.tempo / 100) : 0;
  let pos = S.live.pos + Math.min(run, 250); // never extrapolate far past the last update
  // Wait mode: the Pi stops just before the next chord of your hand, so don't
  // draw past it (otherwise the notes overshoot and snap back when it stops).
  if (S.live.playing && S.live.mode === 'wait' && S.song) {
    for (let i = firstFrom(S.live.pos + 1); i < S.song.notes.length; i++) {
      if (S.song.notes[i][0] > pos) break;
      if (practised(S.song.notes[i])) {
        pos = Math.min(pos, S.song.notes[i][0] - 0.5);
        break;
      }
    }
  }
  // Small steps backwards read as jitter; real jumps (seek, loop) still snap.
  if (pos < shownPos && shownPos - pos < 400) pos = shownPos;
  shownPos = pos;
  return pos;
}

export function practised(n) {
  if (!S.live || S.live.mode === 'listen' || n[4]) return false;
  return S.live.hand === 'both' || (S.live.hand === 'right') === (n[3] === 0);
}

export function handColor(hand) {
  if (!S.settings) return hand ? '#ff8a1f' : '#2f7cff';
  return hand ? S.settings.left_color : S.settings.right_color;
}

// Index of the first note starting at or after `t` (notes are sorted by start).
export function firstFrom(t) {
  const notes = S.song.notes;
  let lo = 0, hi = notes.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (notes[mid][0] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function draw() {
  requestAnimationFrame(draw);
  tickMetronome();
  if (!W || !$('#player').classList.contains('active')) return;
  ctx.clearRect(0, 0, W, H);
  const pos = displayPos();
  S.curSf = S.song ? keyAt(pos) : 0;

  const active = new Map(); // key -> hand of the song note sounding now
  const expected = new Set(); // keys you should be playing now (same window as the Pi)
  if (S.song) {
    const notes = S.song.notes;
    for (let i = firstFrom(pos - S.maxDur); i < notes.length; i++) {
      const n = notes[i];
      const [t, dur, key, hand] = n;
      if (t > pos + 300) break;
      if (t + dur < pos) continue;
      // While waiting, light the chord you're expected to play.
      const ahead = S.live && S.live.waiting && practised(n) ? 61 : 0;
      // Chord keys you already played stay done while the song waits.
      const done = S.live && S.live.waiting && S.live.done && S.live.done.includes(key);
      if (t <= pos + ahead && !done) active.set(key, hand);
      if (practised(n) && t <= pos + (S.live.waiting ? 61 : 250)) expected.add(key);
    }
    if (S.settings && S.settings.view_mode === 'sheet') drawSheet(pos);
    else drawRoll(pos);
  }
  drawKeyboard(active, expected);
  if (S.live) $('#time').textContent = `${fmt(pos)} / ${fmt(S.live.len)}`;
}

// Waterfall: notes fall onto the keyboard.
export function drawRoll(pos) {
  const pxPerMs = keyTop / LOOKAHEAD_MS;
  const names = S.settings && S.settings.note_names;
  // Octave guide lines at every C.
  ctx.fillStyle = '#161b26';
  for (let n = 24; n <= LAST; n += 12) ctx.fillRect(keys[n].x, 0, 1, keyTop);
  const notes = S.song.notes;
  for (let i = firstFrom(pos - S.maxDur); i < notes.length; i++) {
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
      const label = noteName(key, true, S.curSf);
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
// Font size (px, bold) at which `text` fits in `width`, capped at `max`.
export function fitFont(text, width, max) {
  ctx.font = `700 ${max}px system-ui, sans-serif`;
  const w = ctx.measureText(text).width;
  return w <= width ? max : Math.max(7, (max * width) / w);
}
export const TREBLE_BOTTOM = 30; // E4
export const BASS_TOP = 26; // A3
export const SHEET_AHEAD_MS = 4000;
export const CHORD_MS = 60; // same chord window as the Pi's wait mode

// Grand staff scrolling right to left past a playhead; right hand on the
// treble staff, left hand on the bass staff.
export function drawSheet(pos) {
  const g = Math.max(5, Math.min(24, keyTop / 22)); // gap between staff lines
  const mid = keyTop / 2;
  const trebleY = (d) => mid - 2 * g - (d - TREBLE_BOTTOM) * (g / 2);
  const bassY = (d) => mid + 2 * g - (d - BASS_TOP) * (g / 2);
  const sf = S.curSf;
  const clefW = 4 * g + (sf ? Math.abs(sf) * 0.9 * g + 0.6 * g : 0);
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
  // Key signature: sharps F C G D A E B, flats B E A D G C F, on their lines.
  const SHARP_STEPS = [38, 35, 39, 36, 33, 37, 34]; // F5 C5 G5 D5 A4 E5 B4
  const FLAT_STEPS = [34, 37, 33, 36, 32, 35, 31]; // B4 E5 A4 D5 G4 C5 F4
  ctx.font = `${1.9 * g}px serif`;
  ctx.textBaseline = 'middle';
  for (let i = 0; i < Math.abs(sf); i++) {
    const step = (sf > 0 ? SHARP_STEPS : FLAT_STEPS)[i];
    const kx = 4.2 * g + i * 0.9 * g;
    const nudge = sf < 0 ? -0.35 * g : 0; // a flat's bowl sits on its line
    ctx.fillText(sf > 0 ? '♯' : '♭', kx, trebleY(step) + nudge);
    ctx.fillText(sf > 0 ? '♯' : '♭', kx, bassY(step - 14) + nudge);
  }
  ctx.textBaseline = 'alphabetic';

  // Playhead.
  ctx.fillStyle = 'rgba(109,139,255,.9)';
  ctx.fillRect(playX - 1, trebleY(42), 2, bassY(14) - trebleY(42));

  const notes = S.song.notes;
  const names = S.settings && S.settings.note_names;
  ctx.save();
  ctx.beginPath();
  ctx.rect(clefW, 0, W - clefW, keyTop);
  ctx.clip();

  // Bar lines (computed on the Pi from the file's time signature).
  ctx.fillStyle = '#59627a';
  ctx.font = `${Math.max(9, 1.1 * g)}px system-ui, sans-serif`;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'bottom';
  (S.song.bars || []).forEach((b, i) => {
    const bx = playX + (b - pos) * pxPerMs;
    if (bx < clefW || bx > W) return;
    ctx.fillRect(Math.round(bx), trebleY(38), 1.5, bassY(18) - trebleY(38));
    ctx.fillText(i + 1, Math.round(bx) + 3, trebleY(38) - 0.3 * g); // measure number
  });
  ctx.textBaseline = 'alphabetic';

  // Head size: larger when the note name is printed inside.
  const rx = g * (names ? 0.95 : 0.66);
  const ry = g * (names ? 0.66 : 0.46);
  // Group visible notes into chords: same staff, starting within CHORD_MS of
  // the chord's first note (the same window the Pi uses for wait mode).
  const chords = [];
  const open = [null, null]; // current chord per staff: 0 treble, 1 bass
  for (let i = firstFrom(pos - behindMs - S.maxDur); i < notes.length; i++) {
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
  layoutChords(chords);
  for (const c of chords) drawChord(c);
  drawBeams(chords);
  drawOttavas(chords);

  // Staff step of each note, octave shift included.
  function steps(c) {
    return c.notes.map((n) => (n.d ?? diatonic(n[2])) + c.shift);
  }

  // Octave lines instead of a tower of ledger lines: very high treble chords
  // are written an octave lower under "8va", very low bass ones an octave
  // higher over "8vb". Then eighths and shorter on one staff within one beat
  // share a beam, all stems one way, the beam level with the furthest tip.
  function layoutChords(list) {
    for (const c of list) {
      const d = c.notes.map((n) => n.d ?? diatonic(n[2]));
      c.shift = c.staff === 0 && Math.min(...d) > 45 ? -7 : c.staff === 1 && Math.max(...d) < 11 ? 7 : 0;
      c.kind = Math.max(...c.notes.map((n) => (n[5] ?? 4) >> 1));
      c.group = null;
    }
    const beats = S.song.beats || [];
    if (beats.length < 2) return; // no beats (timecode file): flags only
    const beatOf = (t) => {
      let lo = 0, hi = beats.length;
      while (lo < hi) {
        const m = (lo + hi) >> 1;
        if (beats[m] <= t) lo = m + 1;
        else hi = m;
      }
      return lo;
    };
    for (const staff of [0, 1]) {
      let run = [];
      const close = () => {
        if (run.length > 1) {
          const mid = staff === 0 ? TREBLE_BOTTOM + 4 : BASS_TOP - 4;
          const all = run.flatMap(steps);
          const avg = all.reduce((a, b) => a + b, 0) / all.length;
          const up = avg < mid;
          const Y = (d) => (staff === 0 ? trebleY(d) : bassY(d));
          const y = up
            ? Math.min(...run.map((c) => Y(Math.max(...steps(c))))) - 3.4 * g
            : Math.max(...run.map((c) => Y(Math.min(...steps(c))))) + 3.4 * g;
          const group = { up, y, chords: run };
          for (const c of run) c.group = group;
        }
        run = [];
      };
      for (const c of list) {
        if (c.staff !== staff) continue;
        const beat = beatOf(c.t);
        if (c.kind < 3 || (run.length && run[0].beat !== beat) || (run.length && run[0].shift !== c.shift)) close();
        if (c.kind >= 3) {
          c.beat = beat;
          run.push(c);
        }
      }
      close();
    }
  }

  function drawBeams(list) {
    const thick = 0.45 * g;
    for (const c of list) {
      const grp = c.group;
      if (!grp || grp.chords[0] !== c) continue; // once per group, from its first chord
      const cs = grp.chords;
      ctx.globalAlpha = cs.some((x) => x.notes.some(practised)) ? 1 : 0.45;
      ctx.fillStyle = handColor(c.staff);
      const inward = grp.up ? 1 : -1; // from the tip towards the heads
      const bar = (x0, x1, level) => {
        const y = grp.y + inward * level * 0.75 * g;
        ctx.fillRect(Math.min(x0, x1), grp.up ? y : y - thick, Math.abs(x1 - x0) + 1, thick);
      };
      bar(cs[0].stemX, cs[cs.length - 1].stemX, 0);
      // Sixteenths: a second beam between neighbours, or a stub towards one.
      cs.forEach((x, i) => {
        if (x.kind < 4) return;
        const next = cs[i + 1];
        if (next && next.kind >= 4) bar(x.stemX, next.stemX, 1);
        else if (!(cs[i - 1] && cs[i - 1].kind >= 4)) bar(x.stemX, x.stemX + (next ? 1 : -1) * 1.2 * g, 1);
      });
    }
    ctx.globalAlpha = 1;
  }

  function drawOttavas(list) {
    ctx.font = `italic 700 ${Math.max(10, 1.2 * g)}px serif`;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    for (const staff of [0, 1]) {
      const y = staff === 0 ? trebleY(50) : bassY(4);
      const label = staff === 0 ? '8va' : '8vb';
      let from = null;
      let to = null;
      const flush = () => {
        if (from === null) return;
        ctx.fillStyle = '#c3c9d8';
        ctx.fillText(label, from - rx, y);
        const w = ctx.measureText(label).width;
        if (to > from - rx + w + 4) {
          ctx.strokeStyle = '#c3c9d8';
          ctx.setLineDash([4, 3]);
          ctx.beginPath();
          ctx.moveTo(from - rx + w + 4, y);
          ctx.lineTo(to + rx, y);
          ctx.lineTo(to + rx, y + (staff === 0 ? 0.8 : -0.8) * g);
          ctx.stroke();
          ctx.setLineDash([]);
        }
        from = to = null;
      };
      for (const c of list) {
        if (c.staff !== staff) continue;
        if (!c.shift) flush();
        else {
          from ??= c.x;
          to = c.x;
        }
      }
      flush();
    }
    ctx.textBaseline = 'alphabetic';
  }

  function drawChord(c) {
    const treble = c.staff === 0;
    const Y = (d) => (treble ? trebleY(d) : bassY(d));
    const midLine = treble ? TREBLE_BOTTOM + 4 : BASS_TOP - 4; // B4 / D3
    const x = playX + (c.t - pos) * pxPerMs;
    const color = handColor(treble ? 0 : 1);
    const mine = c.notes.some(practised);
    // Heads from bottom to top, with their staff step.
    const heads = c.notes.map((n) => ({ n, d: (n.d ?? diatonic(n[2])) + c.shift })).sort((a, b) => a.d - b.d);
    const bottom = heads[0].d;
    const top = heads[heads.length - 1].d;
    // The note furthest from the middle line decides: above -> stem down (left),
    // below -> stem up (right). Equal distance -> down. A beam decides for its group.
    const up = c.group ? c.group.up : midLine - bottom > top - midLine;
    const stemX = up ? x + rx * 0.92 : x - rx * 0.92;
    c.x = x;
    c.stemX = stemX;
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
    const kind = c.kind;
    ctx.globalAlpha = mine ? 1 : 0.45;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    if (kind >= 1) {
      const from = up ? Y(bottom) : Y(top);
      const tip = c.group ? c.group.y : up ? Y(top) - 3.4 * g : Y(bottom) + 3.4 * g;
      ctx.lineWidth = Math.max(1.2, g * 0.12);
      ctx.beginPath();
      ctx.moveTo(stemX, from);
      ctx.lineTo(stemX, tip);
      for (let f = 0; !c.group && f < kind - 2; f++) {
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
        const letter = n.letter ?? noteName(key, false);
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
      const acc = n.acc ?? SPELL[n[2] % 12][2];
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
export function drawKeyboard(active, expected) {
  const held = new Set(S.live ? S.live.held : []);
  const names = S.settings && S.settings.note_names;
  const practising = S.live && S.live.playing && S.song && S.live.mode !== 'listen';
  const keyH = H - keyTop;
  for (const pass of [false, true]) {
    for (let n = FIRST; n <= LAST; n++) {
      const k = keys[n];
      if (k.black !== pass) continue;
      const h = pass ? keyH * 0.62 : keyH;
      let fill = pass ? '#11141b' : '#e9ecf3';
      if (active.has(n)) fill = shade(handColor(active.get(n)), pass ? 0.75 : 0.55);
      // Wait mode: a key held over from an earlier chord stays "to play" until it's pressed again.
      const required = S.live && S.live.waiting && expected.has(n) && !(S.live.done || []).includes(n);
      if (held.has(n) && !required) fill = !practising || expected.has(n) ? '#3ddc97' : '#ff5a5a';
      ctx.fillStyle = fill;
      ctx.fillRect(k.x + (pass ? 0 : 0.5), keyTop, k.w - (pass ? 0 : 1), h);
      // Note names on the keys to play now.
      if (names && active.has(n)) {
        ctx.fillStyle = pass ? '#ffffff' : '#11141b';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        const label = noteName(n, true, S.curSf);
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

export function roundRect(x, y, w, h, r) {
  if (h <= 0) return;
  r = Math.min(r, h / 2, w / 2);
  ctx.beginPath();
  ctx.roundRect ? ctx.roundRect(x, y, w, h, r) : ctx.rect(x, y, w, h);
  ctx.fill();
}

// Mix a #rrggbb colour towards white (amount 0..1 keeps that much colour).
export function shade(hex, amount) {
  const v = parseInt(hex.slice(1), 16);
  const mix = (c) => Math.round(c * amount + 255 * (1 - amount));
  return `rgb(${mix(v >> 16)},${mix((v >> 8) & 255)},${mix(v & 255)})`;
}
