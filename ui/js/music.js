// Note and chord names: spelling by key signature, intervals, chord symbols.
import { S } from './state.js';

// Spelling follows the key signature (`sf` sharps, negative for flats).
export const LETTERS = 'CDEFGAB';
export const NATURAL = [0, 2, 4, 5, 7, 9, 11]; // pitch class per letter
export const SHARP_ORDER = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B; flats go the other way
export const ACC_SIGN = { 1: '♯', '-1': '♭', 0: '♮' };
export const keyAlterCache = {};
// Alteration (-1, 0, 1) of each letter in key `sf`.
export function keyAlter(sf) {
  if (keyAlterCache[sf]) return keyAlterCache[sf];
  const a = [0, 0, 0, 0, 0, 0, 0];
  for (let i = 0; i < Math.abs(sf); i++) a[sf > 0 ? SHARP_ORDER[i] : SHARP_ORDER[6 - i]] = Math.sign(sf);
  return (keyAlterCache[sf] = a);
}
// Pitch class `pc` in key `sf` as [letter index, alteration]: the key's own
// note first (F♯ in G, E♯ in F♯), then a natural, then a sharp in sharp keys
// or a flat in flat keys. In C: C♯ E♭ F♯ A♭ B♭.
export function spellPc(pc, sf) {
  const ka = keyAlter(sf);
  const dir = sf > 0 ? 1 : sf < 0 ? -1 : pc === 1 || pc === 6 ? 1 : -1;
  let best = null;
  for (let l = 0; l < 7; l++) {
    const a = ((pc - NATURAL[l] + 18) % 12) - 6;
    if (Math.abs(a) > 1) continue;
    const rank = a === ka[l] ? 0 : a === 0 ? 1 : a === dir ? 2 : 3;
    if (!best || rank < best[2]) best = [l, a, rank];
  }
  return best;
}
// C major spelling per pitch class: [letter, letter index C=0..B=6, accidental].
export const SPELL = Array.from({ length: 12 }, (_, pc) => {
  const [l, a] = spellPc(pc, 0);
  return [LETTERS[l], l, a ? ACC_SIGN[a] : ''];
});
// Letter name of `key` in key `sf`, with or without its sharp/flat.
export function noteName(key, withAcc = true, sf = 0) {
  const [l, a] = spellPc(key % 12, sf);
  return LETTERS[l] + (withAcc && a ? ACC_SIGN[a] : '');
}
// Key signature at song time `t`.
export function keyAt(t) {
  const ks = S.song?.keys;
  if (!ks?.length) return 0;
  let sf = ks[0][1];
  for (const [kt, s] of ks) if (kt <= t) sf = s;
  return sf;
} // key at the playhead, for note names on the waterfall and keys
// Per note, as written music has it: the staff step (`d`), the letter, and
// the sign to print (`acc`): only where the note differs from the key
// signature or from an earlier note on that line in the same bar.
export function prepareSpelling(s) {
  const keys = s.keys?.length ? s.keys : [[0, 0]];
  const bars = s.bars || [];
  const staves = [{ bar: -1, sf: null, alt: new Map() }, { bar: -1, sf: null, alt: new Map() }];
  let ki = 0;
  let bi = 0;
  for (const n of s.notes) {
    const [t, , key, hand] = n;
    while (ki + 1 < keys.length && keys[ki + 1][0] <= t) ki++;
    while (bi < bars.length && bars[bi] <= t) bi++;
    const sf = keys[ki][1];
    const [l, a] = spellPc(key % 12, sf);
    const d = (Math.floor((key - a) / 12) - 1) * 7 + l; // B♯/C♭ belong to their letter's octave
    const st = staves[hand === 0 ? 0 : 1];
    if (st.bar !== bi || st.sf !== sf) Object.assign(st, { bar: bi, sf }).alt.clear();
    const now = st.alt.has(d) ? st.alt.get(d) : keyAlter(sf)[l];
    n.d = d;
    n.letter = LETTERS[l];
    n.acc = a === now ? '' : ACC_SIGN[a];
    st.alt.set(d, a);
  }
}
// Diatonic step number (C4 = 28): one step per staff line or space.
export const diatonic = (key) => (Math.floor(key / 12) - 1) * 7 + SPELL[key % 12][1];
export const fullName = (key) => noteName(key) + (Math.floor(key / 12) - 1);
export const KEY_NAMES = ['C', 'D♭', 'D', 'E♭', 'E', 'F', 'F♯', 'G', 'A♭', 'A', 'B♭', 'B'];
export const SHAPE_NAMES = ['', 'm', 'dim', 'aug', '7', 'maj7', 'm7', 'm7♭5', 'sus2', 'sus4', '5']; // same order as the Pi
export const chordName = (c) => (c ? KEY_NAMES[c[0]] + SHAPE_NAMES[c[1]] : '');
