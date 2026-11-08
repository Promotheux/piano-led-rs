<script>
  // Edit a song's title, artist, difficulty and file name. Created while
  // editing; a native modal dialog, so it takes focus and Escape closes it.
  import { untrack } from 'svelte';
  import { api, prettyName, toast } from '/js/util.js';
  import { LEVELS, loadSongs } from './store.svelte.js';

  // heading: "Edit song", or "Name the new song" right after an upload.
  // onclose(name) gets the song's file name after a rename (or the old one).
  let { song, heading = 'Edit song', onclose } = $props();

  // The form starts from the song as it was when opened, on purpose: the list
  // may refresh meanwhile, and that must not overwrite what you are typing.
  const start = untrack(() => song);
  const name = start.name;
  const ext = name.match(/\.midi?$/i)[0];
  const automatic = start.difficulty ? `Automatic (${LEVELS[start.difficulty]})` : 'Automatic';
  let title = $state(start.title || '');
  let artist = $state(start.artist || '');
  let difficulty = $state(start.difficulty_set || '');
  let file = $state(name.slice(0, -ext.length));
  let finalName = name;
  let closed = false;

  // Tell the Library once, right away: the dialog's own close event can be
  // held back while the page isn't drawing (a background tab, screen off).
  function finish() {
    if (closed) return;
    closed = true;
    onclose(finalName);
  }

  function close(dialog) {
    dialog.close();
    finish();
  }

  async function save(e) {
    e.preventDefault();
    const dialog = e.currentTarget.closest('dialog');
    try {
      const r = await api(`/api/songs?name=${encodeURIComponent(name)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title, artist, difficulty_set: difficulty || null, rename: file.trim() + ext }),
      });
      finalName = r.name;
      await loadSongs();
      toast('Saved');
      close(dialog);
    } catch (err) {
      toast(err.message, true); // keep the dialog open to fix it
    }
  }
</script>

<dialog {@attach (d) => d.showModal()} aria-labelledby="edit-heading" onclose={finish}>
  <h3 id="edit-heading">{heading}</h3>
  <form class="editform" onsubmit={save}>
    <label
      >Title<input type="text" maxlength="200" autocomplete="off" placeholder={prettyName(name)} bind:value={title} /></label
    >
    <label>Artist<input type="text" maxlength="200" autocomplete="off" bind:value={artist} /></label>
    <label
      >Difficulty<select bind:value={difficulty}>
        <option value="">{automatic}</option>
        {#each Object.entries(LEVELS) as [v, label] (v)}
          <option value={v}>{label}</option>
        {/each}
      </select></label
    >
    <label
      >File name<span class="file"
        ><input type="text" maxlength="190" autocomplete="off" required bind:value={file} /><span>{ext}</span></span
      ></label
    >
    <div class="actions">
      <button type="button" class="btn ghost" onclick={(e) => close(e.currentTarget.closest('dialog'))}>Cancel</button>
      <button type="submit" class="btn primary">Save</button>
    </div>
  </form>
</dialog>
