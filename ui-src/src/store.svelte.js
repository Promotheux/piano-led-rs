// Library state shared by the Svelte components and the vanilla app.
// The vanilla modules keep S (state.js) as their source of truth; this mirrors
// the parts the Library shows, as runes, so the list re-renders by itself.
import { S } from '/js/state.js';
import { api, songTitle, toast } from '/js/util.js';

export const LEVELS = { beginner: 'Beginner', easy: 'Easy', intermediate: 'Intermediate', advanced: 'Advanced', expert: 'Expert' };

export const lib = $state({
  songs: [],
  stats: {}, // song name -> { finished, best_tempo, best_accuracy }
  current: null, // name of the loaded song
  record: null, // { recording, ms, notes }
});

export async function loadSongs() {
  try {
    S.songs = (await api('/api/songs')).songs;
    S.stats = await api('/api/stats').catch(() => ({}));
    lib.songs = S.songs;
    lib.stats = S.stats;
    if (S.song) document.querySelector('#title').textContent = playerTitle(S.song.name);
  } catch (e) {
    toast(e.message, true);
  }
}

/** The scorecard refreshed S.stats: show the new best tempo. */
export function renderSongs() {
  lib.stats = S.stats;
}

/** Called with every live update from the Pi (live.js). */
export function libraryLive(live) {
  lib.current = live.song;
  lib.record = live.record;
}

/** "Title · Artist" for the player bar. */
export function playerTitle(name) {
  const s = (S.songs || []).find((x) => x.name === name) || { name };
  return s.artist ? `${songTitle(s)} · ${s.artist}` : songTitle(s);
}

export const levelOf = (s) => s.difficulty_set || s.difficulty;

// Natural order, ignoring case: "Song 2" before "Song 10".
const byText = (a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
// Easiest first; your own setting wins over the rating; unrated last.
const levelRank = (s) => {
  const i = Object.keys(LEVELS).indexOf(levelOf(s));
  return i < 0 ? 99 : i;
};
// Title A–Z, artist A–Z then title (songs without an artist last), or
// difficulty then title.
export function sortSongs(songs, by) {
  return [...songs].sort((a, b) => {
    if (by === 'difficulty') return levelRank(a) - levelRank(b) || byText(songTitle(a), songTitle(b));
    if (by === 'artist' && !a.artist !== !b.artist) return a.artist ? -1 : 1;
    return (by === 'artist' && a.artist ? byText(a.artist, b.artist) : 0) || byText(songTitle(a), songTitle(b));
  });
}
