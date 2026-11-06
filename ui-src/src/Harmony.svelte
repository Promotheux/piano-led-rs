<script>
  // Apps: chord progressions. The Pi names (or plays) the progression and
  // judges the chords you hold; this shows the numerals and how you did.
  import { chordName, KEY_NAMES } from '/js/music.js';
  import { apps, command } from './apps.svelte.js';
  import Seg from './Seg.svelte';

  const harmony = command('/api/harmony');
  const PROMPTS = [['symbol', 'Read it'], ['sound', 'By ear']];
  const PROGRESSIONS = [
    ['basic', 'I–IV–V–I'], ['pop', 'I–V–vi–IV'], ['fifties', 'I–vi–IV–V'],
    ['jazz', 'ii–V–I'], ['blues', '12-bar blues'], ['minor', 'i–VI–III–VII'],
  ];
  const ROMAN = ['I', '♭II', 'II', '♭III', 'III', 'IV', '♯IV', 'V', '♭VI', 'VI', '♭VII', 'VII'];

  // A step [semitones above the key, shape] as a Roman numeral. In a minor key
  // III, VI and VII are the scale's own chords, so they lose the flat.
  function numeral([deg, shape], minor) {
    let r = minor && [3, 8, 10].includes(deg) ? ROMAN[deg].slice(1) : ROMAN[deg];
    if ([1, 2, 6, 7].includes(shape)) r = r.toLowerCase(); // minor and diminished
    return r + ({ 2: '°', 3: '+', 4: '7', 5: 'maj7', 6: '7', 7: 'ø7' }[shape] || '');
  }

  const m = $derived(apps.harmony);
  const minor = $derived(m.progression === 'minor');
  const help = $derived(
    !m.active
      ? 'Play each chord in any voicing or inversion. Root and quality count, not the exact notes.'
      : m.playing
        ? 'Listen…'
        : m.step >= m.steps.length
          ? 'Well done. Next one coming…'
          : `In ${KEY_NAMES[m.key]} ${minor ? 'minor' : 'major'}: play the highlighted chord.`,
  );

  function pickKey(e) {
    const v = e.currentTarget.value;
    harmony(v === 'r' ? { random_key: true } : { random_key: false, key: Number(v) });
  }
</script>

<div class="trainer">
  <Seg label="How it asks" options={PROMPTS} value={m.prompt} onpick={(v) => harmony({ prompt: v })} />
  <p class="tr-prompt">{help}</p>
  <p class="hm-steps" aria-live="polite">
    {#if m.active}
      {#each m.steps as st, i (i)}
        {@const mark = m.marks[i]}
        <span class={[mark, { now: i === m.step && !m.playing }]}
          ><!-- By ear: no numerals until you get them. -->{m.prompt === 'sound' && mark === 'open' ? '?' : numeral(st, minor)}<small
            >{mark === 'open' ? ' ' : chordName([(m.key + st[0]) % 12, st[1]])}</small
          ></span
        >
      {/each}
    {/if}
  </p>
  <p class="tr-keys">{m.active && m.heard ? `You're playing ${chordName(m.heard)}` : ' '}</p>
  <div class="row">
    <button class="btn primary" onclick={() => harmony({ active: !m.active })}>{m.active ? 'Stop' : 'Start'}</button>
    <button class="btn" disabled={!m.active || m.playing} onclick={() => harmony({ replay: true })}>Play again</button>
  </div>
  <div class="row wrap">
    <label class="inline"
      >Key <select value={m.random_key ? 'r' : m.key} onchange={pickKey}>
        {#each KEY_NAMES as n, i (i)}<option value={i}>{n}</option>{/each}
        <option value="r">Random</option>
      </select></label
    >
    <Seg small label="Progression" options={PROGRESSIONS} value={m.progression} onpick={(v) => harmony({ progression: v })} />
  </div>
  <dl class="tr-stats">
    <dt>Streak</dt><dd>{m.streak}</dd>
    <dt>Best</dt><dd>{m.best}</dd>
    <dt>Right</dt><dd>{m.total ? `${m.right}/${m.total} (${Math.round((100 * m.right) / m.total)}%)` : '–'}</dd>
  </dl>
  <button class="btn ghost" onclick={() => harmony({ reset: true })}>Reset score</button>
</div>
