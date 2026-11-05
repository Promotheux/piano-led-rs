// Apps: the pitch trainer.
import { S } from './state.js';
import { $, post, setSeg, toast } from './util.js';
import { fullName, LETTERS, NATURAL, SPELL } from './music.js';

// The Pi plays a note, interval or chord and judges the answer; the tablet
// names it afterwards and shows the score.
export const trainer = (cmd) => post('/api/trainer', cmd).catch((e) => toast(e.message, true));
export const INTERVALS = ['', 'Minor 2nd', 'Major 2nd', 'Minor 3rd', 'Major 3rd', 'Perfect 4th', 'Tritone',
  'Perfect 5th', 'Minor 6th', 'Major 6th', 'Minor 7th', 'Major 7th', 'Octave'];
export const CHORDS = ['major', 'minor', 'diminished', 'augmented']; // same order as the Pi
export const TR_ASK = {
  note: ['The piano plays a note. Find it on the keyboard.', 'Which note was that? Play it.'],
  interval: ['The piano plays two notes, one after the other. Play them back.', 'Play back both notes.'],
  chord: ['The piano plays a chord. Play it back.', 'Play back the chord.'],
};
// Spelling inside intervals and chords goes by letter: a third is two
// letters up, a fifth four (so C♯ minor is C♯ E G♯, not C♯ E A♭).
export const ACC = { 0: '', 1: '♯', 2: '𝄪', 11: '♭', 10: '𝄫' };
export function spellAt(key, letter) {
  const l = ((letter % 7) + 7) % 7;
  const pc = key % 12;
  // B♯ and C♭ count to the octave of their letter.
  const oct = Math.floor(key / 12) - 1 + (l === 6 && pc < 2 ? -1 : 0) + (l === 0 && pc > 9 ? 1 : 0);
  return LETTERS[l] + ACC[(pc - NATURAL[l] + 12) % 12] + oct;
}
// Letters spanned by each interval (semitones 0..12).
export const INTERVAL_LETTERS = [0, 1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 6, 7];
// Root letter per pitch class, spelt flat (D♭) or sharp (C♯).
export const FLAT_ROOT = [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6];
export const SHARP_ROOT = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
// The usual root per chord quality: D♭ major, C♯ minor, but E♭ and B♭ minor.
export function chordRoot(pc, kind) {
  if (kind === 0) return pc === 6 ? SHARP_ROOT[pc] : FLAT_ROOT[pc];
  if (kind === 1) return pc === 3 || pc === 10 ? FLAT_ROOT[pc] : SHARP_ROOT[pc];
  return kind === 2 ? SHARP_ROOT[pc] : FLAT_ROOT[pc];
}
export function trName(t) {
  const k = t.shown;
  if (t.drill === 'interval') {
    const l = SPELL[k[0] % 12][1];
    // A tritone has no one right spelling: keep the everyday one (C–F♯, C♯–G).
    const top = t.kind === 6 ? fullName(k[1]) : spellAt(k[1], l + INTERVAL_LETTERS[t.kind]);
    return [INTERVALS[t.kind], `${spellAt(k[0], l)} – ${top}`];
  }
  if (t.drill === 'chord') {
    const l = chordRoot(k[0] % 12, t.kind);
    const names = k.map((key, i) => spellAt(key, l + 2 * i));
    return [`${names[0].replace(/-?\d+$/, '')} ${CHORDS[t.kind]}`, names.join(' ')];
  }
  return [fullName(k[0]), ''];
}
export function renderTrainer() {
  const t = S.live.trainer;
  if (!t) return;
  $('#tr-start').textContent = t.active ? 'Stop' : 'Start';
  $('#tr-replay').disabled = !t.active;
  setSeg('#tr-drill', t.drill);
  setSeg('#tr-range', t.adaptive ? '' : `${t.lo}-${t.hi}`);
  for (const [id, on] of [['#tr-ref', t.reference], ['#tr-auto', t.adaptive]]) {
    $(id).classList.toggle('on', on);
    $(id).setAttribute('aria-pressed', on);
  }
  $('#tr-rangenow').textContent = `Range ${fullName(t.lo)} – ${fullName(t.hi)}` + (t.adaptive ? ', grows with your streak' : '');
  $('#tr-streak').textContent = t.streak;
  $('#tr-best').textContent = t.best;
  $('#tr-score').textContent = t.total ? `${t.right}/${t.total} (${Math.round((100 * t.right) / t.total)}%)` : '–';
  const known = t.active && t.shown.length > 0;
  const [name, keys] = known ? trName(t) : [t.active ? '?' : ' ', ''];
  const note = $('#tr-note');
  note.className = 'tr-note' + (known ? (t.missed ? ' wrong' : ' right') : '');
  note.textContent = name;
  $('#tr-keys').textContent = keys;
  $('#tr-prompt').textContent = !t.active ? (t.reference ? 'First C4 as a reference. ' : '') + TR_ASK[t.drill][0]
    : !known ? TR_ASK[t.drill][1]
    : t.missed ? 'Not quite. Play the lit keys to go on.' : 'Right!';
}
$('#tr-ref').addEventListener('click', () => S.live && trainer({ reference: !S.live.trainer.reference }));
$('#tr-auto').addEventListener('click', () => S.live && trainer({ adaptive: !S.live.trainer.adaptive }));
$('#tr-drill').addEventListener('click', (e) => e.target.dataset.v && trainer({ drill: e.target.dataset.v }));
$('#tr-start').addEventListener('click', () => S.live && trainer({ active: !S.live.trainer.active }));
$('#tr-replay').addEventListener('click', () => trainer({ replay: true }));
$('#tr-reset').addEventListener('click', () => trainer({ reset: true }));
$('#tr-range').addEventListener('click', (e) => {
  const v = e.target.dataset.v;
  if (!v) return;
  const [lo, hi] = v.split('-').map(Number);
  trainer({ lo, hi });
});
