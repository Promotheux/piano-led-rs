// The song library: list, search, upload, record, edit.
import { S } from './state.js';
import { $, api, escapeHtml, player, post, prettyName, songTitle, toast } from './util.js';
import { show } from './nav.js';

export async function loadSongs() {
  try {
    S.songs = (await api('/api/songs')).songs;
    S.stats = await api('/api/stats').catch(() => ({}));
    renderSongs();
    if (S.song) $('#title').textContent = playerTitle(S.song.name);
  } catch (e) {
    toast(e.message, true);
  }
}
/** "Title · Artist" for the player bar. */
export function playerTitle(name) {
  const s = (S.songs || []).find((x) => x.name === name) || { name };
  return s.artist ? `${songTitle(s)} · ${s.artist}` : songTitle(s);
}
// Natural order, ignoring case: "Song 2" before "Song 10".
const byText = (a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
// Title A–Z, or artist A–Z then title (songs without an artist last).
export function sortSongs(songs, by) {
  return [...songs].sort((a, b) => {
    if (by === 'artist' && !a.artist !== !b.artist) return a.artist ? -1 : 1;
    return (by === 'artist' && a.artist ? byText(a.artist, b.artist) : 0) || byText(songTitle(a), songTitle(b));
  });
}
$('#sort').value = localStorage.getItem('sort') || 'title';
$('#sort').addEventListener('change', () => {
  localStorage.setItem('sort', $('#sort').value);
  renderSongs();
});

export function renderSongs() {
  const q = $('#search').value.trim().toLowerCase();
  const ul = $('#songs');
  ul.innerHTML = '';
  for (const s of sortSongs(S.songs, $('#sort').value)) {
    if (q && ![s.name, s.title, s.artist].some((t) => t && t.toLowerCase().includes(q))) continue;
    const title = escapeHtml(songTitle(s));
    const li = document.createElement('li');
    li.dataset.name = s.name;
    li.classList.toggle('current', S.live && S.live.song === s.name);
    li.innerHTML = `<span class="name">${title}${s.artist ? `<span class="artist">${escapeHtml(s.artist)}</span>` : ''}</span>` +
      `${S.stats[s.name] ? `<span class="best" title="Best tempo you finished it at">★ ${S.stats[s.name].best_tempo}%</span>` : ''}` +
      `<span class="size">${Math.max(1, Math.round(s.size / 1024))} KB</span>` +
      `<button class="icon-btn edit" aria-label="Edit ${title}">✎</button>` +
      `<button class="icon-btn del" aria-label="Delete ${title}">🗑</button>`;
    li.addEventListener('click', async () => {
      await player({ song: s.name });
      show('player');
    });
    li.querySelector('.edit').addEventListener('click', (e) => {
      e.stopPropagation(); // don't open the song
      openEdit(s);
    });
    li.querySelector('.del').addEventListener('click', async (e) => {
      e.stopPropagation();
      if (!confirm(`Delete "${songTitle(s)}"? This removes the file from the Pi.`)) return;
      try {
        await api(`/api/songs?name=${encodeURIComponent(s.name)}`, { method: 'DELETE' });
        toast(`Deleted ${songTitle(s)}`);
        loadSongs();
      } catch (err) {
        toast(err.message, true);
      }
    });
    ul.append(li);
  }
}

let editing = null;
function openEdit(s) {
  editing = s;
  const ext = s.name.match(/\.midi?$/i)[0];
  $('#edit-title').value = s.title || '';
  $('#edit-title').placeholder = prettyName(s.name);
  $('#edit-artist').value = s.artist || '';
  $('#edit-file').value = s.name.slice(0, -ext.length);
  $('#edit-ext').textContent = ext;
  $('#edit').showModal(); // native modal: focuses the first field, Escape closes
}
$('#edit-cancel').addEventListener('click', () => $('#edit').close());
$('#editform').addEventListener('submit', async (e) => {
  e.preventDefault();
  const s = editing;
  const rename = $('#edit-file').value.trim() + $('#edit-ext').textContent;
  try {
    await api(`/api/songs?name=${encodeURIComponent(s.name)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: $('#edit-title').value, artist: $('#edit-artist').value, rename }),
    });
    $('#edit').close();
    toast('Saved');
    loadSongs();
  } catch (err) {
    toast(err.message, true); // keep the dialog open to fix it
  }
});

$('#search').addEventListener('input', renderSongs);
$('#record').addEventListener('click', async () => {
  if (!S.live) return;
  try {
    const r = await post('/api/record', { recording: !S.live.record.recording });
    if (r.recording) toast('Recording: play away, tap Stop when done');
    else if (r.saved) {
      toast(`Saved as "${prettyName(r.saved)}"`);
      loadSongs();
    } else toast('Nothing was played, so nothing was saved');
  } catch (e) {
    toast(e.message, true);
  }
});
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
