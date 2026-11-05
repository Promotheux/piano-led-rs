// The song library: list, search, upload, record.
import { S } from './state.js';
import { $, api, escapeHtml, player, post, prettyName, toast } from './util.js';
import { show } from './nav.js';

 // song name -> { finished, best_tempo, best_accuracy }
export async function loadSongs() {
  try {
    S.songs = (await api('/api/songs')).songs;
    S.stats = await api('/api/stats').catch(() => ({}));
    renderSongs();
  } catch (e) {
    toast(e.message, true);
  }
}
export function renderSongs() {
  const q = $('#search').value.trim().toLowerCase();
  const ul = $('#songs');
  ul.innerHTML = '';
  for (const s of S.songs) {
    if (q && !s.name.toLowerCase().includes(q)) continue;
    const li = document.createElement('li');
    li.dataset.name = s.name;
    li.classList.toggle('current', S.live && S.live.song === s.name);
    li.innerHTML = `<span class="name">${escapeHtml(prettyName(s.name))}</span>${S.stats[s.name] ? `<span class="best" title="Best tempo you finished it at">★ ${S.stats[s.name].best_tempo}%</span>` : ''}` +
      `<span class="size">${Math.max(1, Math.round(s.size / 1024))} KB</span>` +
      `<button class="icon-btn del" aria-label="Delete ${escapeHtml(prettyName(s.name))}">🗑</button>`;
    li.addEventListener('click', async () => {
      await player({ song: s.name });
      show('player');
    });
    li.querySelector('.del').addEventListener('click', async (e) => {
      e.stopPropagation(); // don't open the song
      if (!confirm(`Delete "${prettyName(s.name)}"? This removes the file from the Pi.`)) return;
      try {
        await api(`/api/songs?name=${encodeURIComponent(s.name)}`, { method: 'DELETE' });
        toast(`Deleted ${prettyName(s.name)}`);
        loadSongs();
      } catch (err) {
        toast(err.message, true);
      }
    });
    ul.append(li);
  }
}
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
