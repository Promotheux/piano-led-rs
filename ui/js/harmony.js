// Apps: chord progressions.
import { S } from './state.js';
import { $, post, setSeg, toast } from './util.js';
import { chordName, KEY_NAMES } from './music.js';

// The Pi names (or plays) the progression and judges the chords you hold;
// the tablet shows the numerals and how you did on each.
export const harmony = (cmd) => post('/api/harmony', cmd).catch((e) => toast(e.message, true));
export const ROMAN = ['I', '♭II', 'II', '♭III', 'III', 'IV', '♯IV', 'V', '♭VI', 'VI', '♭VII', 'VII'];
// A step [semitones above the key, shape] as a Roman numeral. In a minor key
// III, VI and VII are the scale's own chords, so they lose the flat.
export function numeral([deg, shape], minor) {
  let r = minor && [3, 8, 10].includes(deg) ? ROMAN[deg].slice(1) : ROMAN[deg];
  if ([1, 2, 6, 7].includes(shape)) r = r.toLowerCase(); // minor and diminished
  return r + ({ 2: '°', 3: '+', 4: '7', 5: 'maj7', 6: '7', 7: 'ø7' }[shape] || '');
}
$('#hm-key').innerHTML =
  KEY_NAMES.map((n, i) => `<option value="${i}">${n}</option>`).join('') + '<option value="r">Random</option>';
export function renderHarmony() {
  const m = S.live.harmony;
  if (!m) return;
  setSeg('#hm-prompt', m.prompt);
  setSeg('#hm-prog', m.progression);
  if (document.activeElement !== $('#hm-key')) $('#hm-key').value = m.random_key ? 'r' : m.key;
  const minor = m.progression === 'minor';
  const done = m.step >= m.steps.length;
  $('#hm-start').textContent = m.active ? 'Stop' : 'Start';
  $('#hm-replay').disabled = !m.active || m.playing;
  $('#hm-help').textContent = !m.active
    ? 'Play each chord in any voicing or inversion. Root and quality count, not the exact notes.'
    : m.playing
      ? 'Listen…'
      : done
        ? 'Well done. Next one coming…'
        : `In ${KEY_NAMES[m.key]} ${minor ? 'minor' : 'major'}: play the highlighted chord.`;
  $('#hm-steps').innerHTML = !m.active
    ? ''
    : m.steps
        .map((st, i) => {
          const mark = m.marks[i];
          const hidden = m.prompt === 'sound' && mark === 'open'; // by ear: no numerals until you get them
          const name = mark === 'open' ? '&nbsp;' : chordName([(m.key + st[0]) % 12, st[1]]);
          return `<span class="${mark}${i === m.step && !m.playing ? ' now' : ''}">${hidden ? '?' : numeral(st, minor)}<small>${name}</small></span>`;
        })
        .join('');
  $('#hm-heard').textContent = m.active && m.heard ? `You're playing ${chordName(m.heard)}` : ' ';
  $('#hm-streak').textContent = m.streak;
  $('#hm-best').textContent = m.best;
  $('#hm-score').textContent = m.total ? `${m.right}/${m.total} (${Math.round((100 * m.right) / m.total)}%)` : '–';
}
$('#hm-start').addEventListener('click', () => S.live && harmony({ active: !S.live.harmony.active }));
$('#hm-replay').addEventListener('click', () => harmony({ replay: true }));
$('#hm-reset').addEventListener('click', () => harmony({ reset: true }));
$('#hm-key').addEventListener('change', (e) =>
  harmony(e.target.value === 'r' ? { random_key: true } : { random_key: false, key: Number(e.target.value) }),
);
for (const [sel, field] of [['#hm-prompt', 'prompt'], ['#hm-prog', 'progression']]) {
  $(sel).addEventListener('click', (e) => e.target.dataset.v && harmony({ [field]: e.target.dataset.v }));
}
