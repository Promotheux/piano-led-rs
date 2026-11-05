// Shared state: what the Pi last pushed, the loaded song, and the few
// values more than one part of the app changes.

export const LOOKAHEAD_MS = 3000;
export const FIRST = 21, LAST = 108;

export const S = {
  live: null, // last state pushed by the Pi
  liveAt: 0, // performance.now() when it arrived
  song: null, // { name, len, tracks, notes: [[t, dur, key, hand, far, value]], bars, beats, keys }
  maxDur: 0,
  settings: null,
  songs: [],
  stats: {}, // song name -> { finished, best_tempo, best_accuracy }
  scrubbing: false,
  loopA: null,
  settingsGen: null, // last settings generation seen from the Pi
  metronome: localStorage.getItem('metronome') === '1',
  curSf: 0, // key at the playhead, for note names on the waterfall and keys
};
