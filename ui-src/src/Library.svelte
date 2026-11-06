<script>
  // The song library: search, sort, filter by difficulty, upload, record,
  // and the list itself. Styles come from ui/app.css (same classes as before).
  import { api, fmt, ICON, player, post, prettyName, songTitle, toast } from '/js/util.js';
  import { show } from '/js/nav.js';
  import { lib, LEVELS, levelOf, loadSongs, sortSongs } from './store.svelte.js';
  import EditSong from './EditSong.svelte';

  let query = $state('');
  let sort = $state(localStorage.getItem('sort') || 'title');
  let level = $state(localStorage.getItem('level') || ''); // '' = all songs
  let editing = $state(null); // the song in the edit dialog

  const counts = $derived.by(() => {
    const c = { '': lib.songs.length };
    for (const s of lib.songs) {
      const l = levelOf(s);
      if (l) c[l] = (c[l] || 0) + 1;
    }
    return c;
  });

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return sortSongs(lib.songs, sort).filter(
      (s) =>
        (!level || levelOf(s) === level) &&
        (!q || [s.name, s.title, s.artist].some((t) => t && t.toLowerCase().includes(q))),
    );
  });

  function setSort() {
    localStorage.setItem('sort', sort);
  }

  function setLevel(v) {
    level = v;
    localStorage.setItem('level', v);
  }

  async function open(s) {
    await player({ song: s.name });
    show('player');
  }

  async function remove(s) {
    if (!confirm(`Delete "${songTitle(s)}"? This removes the file from the Pi.`)) return;
    try {
      await api(`/api/songs?name=${encodeURIComponent(s.name)}`, { method: 'DELETE' });
      toast(`Deleted ${songTitle(s)}`);
      loadSongs();
    } catch (err) {
      toast(err.message, true);
    }
  }

  async function toggleRecord() {
    if (!lib.record) return;
    try {
      const r = await post('/api/record', { recording: !lib.record.recording });
      if (r.recording) toast('Recording: play away, tap Stop when done');
      else if (r.saved) {
        toast(`Saved as "${prettyName(r.saved)}"`);
        loadSongs();
      } else toast('Nothing was played, so nothing was saved');
    } catch (e) {
      toast(e.message, true);
    }
  }

  async function upload(e) {
    for (const f of e.currentTarget.files) {
      try {
        await api(`/api/songs?name=${encodeURIComponent(f.name)}`, { method: 'POST', body: f });
        toast(`Added ${prettyName(f.name)}`);
      } catch (err) {
        toast(`${f.name}: ${err.message}`, true);
      }
    }
    e.currentTarget.value = '';
    loadSongs();
  }
</script>

<div class="toolbar">
  <input type="search" placeholder="Search songs…" autocomplete="off" aria-label="Search songs" bind:value={query} />
  <select aria-label="Sort songs" bind:value={sort} onchange={setSort}>
    <option value="title">Title A–Z</option>
    <option value="artist">Artist A–Z</option>
    <option value="difficulty">Difficulty (easiest first)</option>
  </select>
  <label class="btn">Upload .mid<input type="file" accept=".mid,.midi,audio/midi" multiple hidden onchange={upload} /></label>
  <button
    id="record"
    class={['btn', { on: lib.record?.recording }]}
    aria-pressed={!!lib.record?.recording}
    title="Record what you play to a new song in the library"
    onclick={toggleRecord}
  >
    {lib.record?.recording ? `■ Stop ${fmt(lib.record.ms)} · ${lib.record.notes} notes` : '● Record'}
  </button>
</div>

<div class="pills" role="group" aria-label="Show songs of one difficulty">
  {#each [['', 'All'], ...Object.entries(LEVELS)] as [v, label] (v)}
    <button class={['pill', { on: level === v }]} aria-pressed={level === v} onclick={() => setLevel(v)}>
      {label}<small>{counts[v] || 0}</small>
    </button>
  {/each}
</div>

<ul class="songlist">
  {#each shown as s (s.name)}
    {@const title = songTitle(s)}
    {@const l = levelOf(s)}
    <!-- The row loads the song; the buttons inside it are separate targets. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <li class={{ current: lib.current === s.name }} onclick={() => open(s)}>
      <div class="info">
        <span class="title">{title}</span>
        <span class="artist">{s.artist || ' '}</span>
      </div>
      <div class="meta">
        <span class="best">
          {#if lib.stats[s.name]}
            <span title="Best tempo you finished it at">★ {lib.stats[s.name].best_tempo}%</span>
          {/if}
        </span>
        <span class="level">
          {#if l}
            <span
              class={['diff', l, { set: s.difficulty_set }]}
              title={s.difficulty_set ? 'Difficulty (set by you)' : 'Difficulty (rated from the notes)'}>{LEVELS[l]}</span
            >
          {/if}
        </span>
        <span class="size">{Math.max(1, Math.round(s.size / 1024))} KB</span>
      </div>
      <!-- Icons are fixed SVG strings from util.js, never song data: safe to inject. -->
      <div class="actions">
        <button
          class="icon-btn edit"
          aria-label="Edit {title}"
          onclick={(e) => {
            e.stopPropagation();
            editing = s;
          }}>{@html ICON.edit}</button
        >
        <button
          class="icon-btn del"
          aria-label="Delete {title}"
          onclick={(e) => {
            e.stopPropagation();
            remove(s);
          }}>{@html ICON.trash}</button
        >
      </div>
    </li>
  {/each}
</ul>

{#if editing}
  <EditSong song={editing} onclose={() => (editing = null)} />
{/if}
