<script>
  // The song library: search, sort, filter by difficulty, upload (button or
  // drag and drop anywhere), record, and the list itself. Styles: ui/app.css.
  import { tick } from 'svelte';
  import { api, fmt, ICON, player, post, prettyName, songTitle, toast } from '/js/util.js';
  import { show } from '/js/nav.js';
  import { lib, LEVELS, levelOf, loadSongs, sortSongs } from './store.svelte.js';
  import EditSong from './EditSong.svelte';

  let query = $state('');
  let sort = $state(localStorage.getItem('sort') || 'title');
  let level = $state(localStorage.getItem('level') || ''); // '' = all songs
  let editing = $state(null); // the song in the edit dialog
  let naming = $state([]); // uploaded songs still to name, in order
  let dragDepth = $state(0); // > 0 while files are dragged over the page

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

  // Uploads the MIDI files, then asks for a name for each new song in turn.
  async function addFiles(files) {
    const added = [];
    for (const f of files) {
      if (!/\.midi?$/i.test(f.name)) {
        toast(`${f.name}: not a MIDI file (.mid)`, true);
        continue;
      }
      try {
        const r = await api(`/api/songs?name=${encodeURIComponent(f.name)}`, { method: 'POST', body: f });
        added.push(r.name);
      } catch (err) {
        toast(`${f.name}: ${err.message}`, true);
      }
    }
    if (!added.length) return;
    await loadSongs();
    toast(added.length === 1 ? `Added ${prettyName(added[0])}` : `Added ${added.length} songs`);
    naming = added;
    nameNext();
  }

  function nameNext() {
    editing = lib.songs.find((s) => s.name === naming[0]) ?? null;
  }

  // The edit dialog closed (saved, or cancelled with Escape).
  async function editClosed(name) {
    const fresh = naming.length > 0;
    editing = null;
    if (!fresh) return;
    naming = naming.slice(1);
    await select(name);
    if (naming.length) nameNext();
  }

  // Loads a new song and shows it in the list: clears a search or filter
  // that would hide it, and scrolls to its row.
  async function select(name) {
    query = '';
    if (level) setLevel('');
    lib.current = name; // highlight now; the Pi confirms within a moment
    await player({ song: name });
    await tick();
    document.querySelector(`#library li[data-name="${CSS.escape(name)}"]`)?.scrollIntoView({ block: 'center' });
  }

  // Drag and drop: files dragged over any tab bring up the Library to drop on.
  const hasFiles = (e) => [...(e.dataTransfer?.types ?? [])].includes('Files');
  function dragEnter(e) {
    if (!hasFiles(e)) return;
    if (dragDepth++ === 0) show('library');
  }
  function dragOver(e) {
    if (hasFiles(e)) e.preventDefault(); // allow the drop (instead of opening the file)
  }
  function drop(e) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    addFiles(e.dataTransfer.files);
  }
</script>

<svelte:window
  ondragenter={dragEnter}
  ondragover={dragOver}
  ondragleave={(e) => hasFiles(e) && (dragDepth = Math.max(0, dragDepth - 1))}
  ondrop={drop}
/>

{#if dragDepth > 0}
  <div class="dropzone" aria-hidden="true"><p>Drop MIDI files to add them to your library</p></div>
{/if}

<div class="toolbar">
  <input type="search" placeholder="Search songs…" autocomplete="off" aria-label="Search songs" bind:value={query} />
  <select aria-label="Sort songs" bind:value={sort} onchange={setSort}>
    <option value="title">Title A–Z</option>
    <option value="artist">Artist A–Z</option>
    <option value="difficulty">Difficulty (easiest first)</option>
  </select>
  <label class="btn"
    >Upload .mid<input
      type="file"
      accept=".mid,.midi,audio/midi"
      multiple
      hidden
      onchange={(e) => {
        addFiles([...e.currentTarget.files]);
        e.currentTarget.value = '';
      }}
    /></label
  >
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
    <li class={{ current: lib.current === s.name }} data-name={s.name} onclick={() => open(s)}>
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
        <a
          class="icon-btn download"
          href="/api/songs/file?name={encodeURIComponent(s.name)}"
          download={s.name}
          aria-label="Download {title}"
          onclick={(e) => e.stopPropagation()}>{@html ICON.download}</a
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
  <EditSong song={editing} heading={naming.length ? 'Name the new song' : 'Edit song'} onclose={editClosed} />
{/if}
