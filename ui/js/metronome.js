// Metronome and count-in clicks, from this tablet.
import { S } from './state.js';
import { $ } from './util.js';
import { displayPos } from './view.js';

export let audio = null;
export let clickedTo = null; // song time up to which clicks are scheduled
export function ensureAudio() {
  // Browsers only allow sound after a tap.
  audio ??= new (window.AudioContext || window.webkitAudioContext)();
  if (audio.state === 'suspended') audio.resume();
}
document.addEventListener('pointerdown', () => (S.metronome || S.live?.count_in) && ensureAudio());
export function showMetronome() {
  $('#metro').classList.toggle('on', S.metronome);
  $('#metro').setAttribute('aria-pressed', S.metronome);
}
showMetronome();
export function click(when, accent) {
  const o = audio.createOscillator();
  const g = audio.createGain();
  o.frequency.value = accent ? 1600 : 1000;
  g.gain.setValueAtTime(0.0001, when);
  g.gain.exponentialRampToValueAtTime(accent ? 0.6 : 0.3, when + 0.002);
  g.gain.exponentialRampToValueAtTime(0.0001, when + 0.05);
  o.connect(g).connect(audio.destination);
  o.start(when);
  o.stop(when + 0.06);
}
// Beat times, with the first beat's spacing carried back into the lead-in.
export function clickTimes() {
  if (S.song.clicks) return S.song.clicks;
  const beats = S.song.beats || [];
  const step = beats.length > 1 ? beats[1] - beats[0] : 500;
  const lead = [];
  for (let t = (beats[0] ?? 0) - step; t >= -2000; t -= step) lead.unshift(t);
  S.song.accents = new Set(S.song.bars || []);
  return (S.song.clicks = [...lead, ...beats]);
}
export function tickMetronome() {
  if (!audio || !S.song || !S.live || !S.live.playing) return void (clickedTo = null);
  if (S.live.waiting) return; // the song stands still: so does the beat
  const counting = S.live.count_in && (displayPos() < 0 || S.live.count_until !== null);
  if (!S.metronome && !counting) return void (clickedTo = null);
  const pos = displayPos();
  if (clickedTo === null || pos < clickedTo - 200) clickedTo = pos; // started, or jumped back
  const speed = S.live.tempo / 100;
  const until = pos + 120 * speed; // schedule a little ahead of the frame clock
  for (const t of clickTimes()) {
    if (t <= clickedTo) continue;
    if (t > until) break;
    const countIn = t < 0 || (S.live.count_until !== null && t < S.live.count_until);
    if (S.metronome || countIn) click(audio.currentTime + Math.max(0, (t - pos) / speed / 1000), S.song.accents.has(t));
  }
  clickedTo = until;
}
