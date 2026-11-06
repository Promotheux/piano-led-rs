// Names for the pitch trainer's questions: notes, intervals and triads,
// spelt by letter (a third is two letters up, so C♯ minor is C♯ E G♯).
import { fullName, LETTERS, NATURAL, SPELL } from '/js/music.js';

export const INTERVALS = ['', 'Minor 2nd', 'Major 2nd', 'Minor 3rd', 'Major 3rd', 'Perfect 4th', 'Tritone',
  'Perfect 5th', 'Minor 6th', 'Major 6th', 'Minor 7th', 'Major 7th', 'Octave'];
export const CHORDS = ['major', 'minor', 'diminished', 'augmented']; // same order as the Pi
export const TR_ASK = {
  note: ['The piano plays a note. Find it on the keyboard.', 'Which note was that? Play it.'],
  interval: ['The piano plays two notes, one after the other. Play them back.', 'Play back both notes.'],
  chord: ['The piano plays a chord. Play it back.', 'Play back the chord.'],
};
const ACC = { 0: '', 1: '♯', 2: '𝄪', 11: '♭', 10: '𝄫' };
function spellAt(key, letter) {
  const l = ((letter % 7) + 7) % 7;
  const pc = key % 12;
  // B♯ and C♭ count to the octave of their letter.
  const oct = Math.floor(key / 12) - 1 + (l === 6 && pc < 2 ? -1 : 0) + (l === 0 && pc > 9 ? 1 : 0);
  return LETTERS[l] + ACC[(pc - NATURAL[l] + 12) % 12] + oct;
}
// Letters spanned by each interval (semitones 0..12).
const INTERVAL_LETTERS = [0, 1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 6, 7];
// Root letter per pitch class, spelt flat (D♭) or sharp (C♯).
const FLAT_ROOT = [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6];
const SHARP_ROOT = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
// The usual root per chord quality: D♭ major, C♯ minor, but E♭ and B♭ minor.
function chordRoot(pc, kind) {
  if (kind === 0) return pc === 6 ? SHARP_ROOT[pc] : FLAT_ROOT[pc];
  if (kind === 1) return pc === 3 || pc === 10 ? FLAT_ROOT[pc] : SHARP_ROOT[pc];
  return kind === 2 ? SHARP_ROOT[pc] : FLAT_ROOT[pc];
}

/** [big name, the keys] for the question just answered. */
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
