<script>
  // Apps: the pitch trainer. The Pi plays a note, interval or chord and
  // judges the answer; this names it afterwards and shows the score.
  import { fullName } from '/js/music.js';
  import { apps, command } from './apps.svelte.js';
  import { trName, TR_ASK } from './trainerNames.js';
  import Seg from './Seg.svelte';

  const trainer = command('/api/trainer');
  const DRILLS = [['note', 'Notes'], ['interval', 'Intervals'], ['chord', 'Chords']];
  const RANGES = [['60-71', 'C4–B4'], ['48-71', 'C3–B4'], ['48-83', 'C3–B5'], ['36-84', 'C2–C6']];

  const t = $derived(apps.trainer);
  const known = $derived(t.active && t.shown.length > 0);
  const [name, keys] = $derived(known ? trName(t) : [t.active ? '?' : ' ', '']);
  const prompt = $derived(
    !t.active
      ? (t.reference ? 'First C4 as a reference. ' : '') + TR_ASK[t.drill][0]
      : !known
        ? TR_ASK[t.drill][1]
        : t.missed
          ? 'Not quite. Play the lit keys to go on.'
          : 'Right!',
  );

  function pickRange(v) {
    const [lo, hi] = v.split('-').map(Number);
    trainer({ lo, hi });
  }
</script>

<div class="trainer">
  <Seg label="Drill" options={DRILLS} value={t.drill} onpick={(v) => trainer({ drill: v })} />
  <p class="tr-prompt">{prompt}</p>
  <p class={['tr-note', known && (t.missed ? 'wrong' : 'right')]} aria-live="polite">{name}</p>
  <p class="tr-keys">{keys}</p>
  <div class="row">
    <button class="btn primary" onclick={() => trainer({ active: !t.active })}>{t.active ? 'Stop' : 'Start'}</button>
    <button class="btn" disabled={!t.active} onclick={() => trainer({ replay: true })}>Play again</button>
  </div>
  <Seg label="Range" options={RANGES} value={t.adaptive ? '' : `${t.lo}-${t.hi}`} onpick={pickRange} />
  <div class="row">
    <button class={['btn ghost toggle', { on: t.reference }]} aria-pressed={t.reference} onclick={() => trainer({ reference: !t.reference })}
      >Reference C4 first</button
    >
    <button class={['btn ghost toggle', { on: t.adaptive }]} aria-pressed={t.adaptive} onclick={() => trainer({ adaptive: !t.adaptive })}
      >Auto range</button
    >
  </div>
  <p class="tr-keys">Range {fullName(t.lo)} – {fullName(t.hi)}{t.adaptive ? ', grows with your streak' : ''}</p>
  <dl class="tr-stats">
    <dt>Streak</dt><dd>{t.streak}</dd>
    <dt>Best</dt><dd>{t.best}</dd>
    <dt>Right</dt><dd>{t.total ? `${t.right}/${t.total} (${Math.round((100 * t.right) / t.total)}%)` : '–'}</dd>
  </dl>
  <button class="btn ghost" onclick={() => trainer({ reset: true })}>Reset score</button>
</div>
