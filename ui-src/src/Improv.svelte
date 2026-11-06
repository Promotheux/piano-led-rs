<script>
  // Apps: improv mode. The Pi recognises (or plays) the chord and lights
  // what fits; this names it and holds the controls.
  import { chordName, fullName, KEY_NAMES } from '/js/music.js';
  import { apps, command } from './apps.svelte.js';
  import { cfg, setSetting } from './settings.svelte.js';
  import Seg from './Seg.svelte';

  const improv = command('/api/improv');
  const SOURCES = [['lead', 'I play the chords'], ['backing', 'Backing']];
  const SCALES = [['major', 'Major'], ['minor', 'Minor'], ['blues', 'Blues']];
  const LIGHTS = [['scale', 'Chord + scale'], ['pentatonic', 'Pentatonic'], ['chord', 'Chord only']];
  const PROGRESSIONS = [['pop', 'I–V–vi–IV'], ['fifties', 'I–vi–IV–V'], ['jazz', 'ii–V–I'], ['blues', '12-bar blues'], ['minor', 'i–VI–III–VII']];

  const m = $derived(apps.improv);
  const backing = $derived(m.source === 'backing');
  // While the tempo slider is dragged, show its value instead of the Pi's.
  let dragTempo = $state(null);
</script>

<div class="trainer">
  <Seg label="Chords from" options={SOURCES} value={m.source} onpick={(v) => improv({ source: v })} />
  <p class="tr-prompt">
    {backing ? 'The piano plays the chords. Improvise with the lit keys.' : `Play chords below ${fullName(m.split)}; improvise above it with the lit keys.`}
  </p>
  <p class="tr-note">{m.active ? chordName(m.chord) || '…' : ' '}</p>
  <p class="tr-keys">{m.active && m.next ? `next: ${chordName(m.next)}` : ''}</p>
  <div class="beats" aria-hidden="true">
    {#each [0, 1, 2, 3] as i (i)}<i class={{ on: m.active && backing && i === m.beat }}></i>{/each}
  </div>
  <button class="btn primary" onclick={() => improv({ active: !m.active })}>{m.active ? 'Stop' : 'Start'}</button>
  <div class="row wrap">
    <label class="inline"
      >Key <select value={m.key} onchange={(e) => improv({ key: Number(e.currentTarget.value) })}>
        {#each KEY_NAMES as n, i (i)}<option value={i}>{n}</option>{/each}
      </select></label
    >
    <!-- A progression brings its own scale. -->
    {#if !backing}
      <Seg small label="Scale" options={SCALES} value={m.scale} onpick={(v) => improv({ scale: v })} />
    {/if}
    <!-- Lights are a setting (kept across restarts): saved as such. -->
    <Seg small label="What lights up" options={LIGHTS} value={m.lights} onpick={(v) => setSetting('improv_lights', v)} />
  </div>
  {#if backing}
    <div class="row wrap">
      <Seg small label="Progression" options={PROGRESSIONS} value={m.progression} onpick={(v) => improv({ progression: v })} />
      <label class="tempo"
        >Tempo <input
          type="range"
          min="40"
          max="200"
          step="5"
          value={dragTempo ?? m.tempo}
          oninput={(e) => (dragTempo = Number(e.currentTarget.value))}
          onchange={(e) => {
            improv({ tempo: Number(e.currentTarget.value) });
            dragTempo = null;
          }}
        /><b>{dragTempo ?? m.tempo}</b></label
      >
    </div>
  {/if}
  <!-- Legend swatches in the strip's own hand colours. -->
  <p class="tr-keys legend">
    <b class="sw chord" style:background={cfg.form?.left_color}></b> chord notes, safe to land on &nbsp;
    <b class="sw scale" style:background={cfg.form?.right_color} style:opacity="0.45"></b> scale notes, for passing
  </p>
</div>
