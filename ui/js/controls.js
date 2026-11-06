// Player controls, track roles and the end-of-song scorecard.
import { S } from './state.js';
import { $, $$, api, escapeHtml, fmt, player, setSeg, toast } from './util.js';
import { displayPos } from './view.js';
import { ensureAudio, showMetronome } from './metronome.js';
import { renderSongs, setSetting } from '/dist/app.js';

$('#play').addEventListener('click', () => S.live && player({ playing: !S.live.playing }));
$('#restart').addEventListener('click', () => player({ seek: -2000 }));
$('#scrub').addEventListener('input', () => (S.scrubbing = true));
$('#scrub').addEventListener('change', (e) => {
  S.scrubbing = false;
  player({ seek: Number(e.target.value) });
});
$('#tempo').addEventListener('input', (e) => ($('#tempoval').textContent = e.target.value + '%'));
$('#tempo').addEventListener('change', (e) => player({ tempo: Number(e.target.value) }));
$$('#mode button').forEach((b) => b.addEventListener('click', () => player({ mode: b.dataset.v })));
$$('#hand button').forEach((b) => b.addEventListener('click', () => player({ hand: b.dataset.v })));
$$('#view button').forEach((b) =>
  b.addEventListener('click', () => {
    if (!S.settings) return;
    setSetting('view_mode', b.dataset.v); // switches now; the save syncs other tablets
    setSeg('#view', b.dataset.v);
  })
);
$('#ramp').addEventListener('click', () => S.live && player({ ramp: !S.live.ramp }));
$('#countin').addEventListener('click', () => {
  ensureAudio();
  if (S.live) player({ count_in: !S.live.count_in });
});
$('#metro').addEventListener('click', () => {
  ensureAudio();
  S.metronome = !S.metronome;
  localStorage.setItem('metronome', S.metronome ? '1' : '');
  showMetronome();
});
$$('#loop button').forEach((b) =>
  b.addEventListener('click', () => {
    const pos = displayPos();
    if (b.dataset.v === 'a') {
      S.loopA = pos;
      toast('Loop start set, now tap B at the end');
    } else if (b.dataset.v === 'b') {
      if (S.loopA === null || pos <= S.loopA) return toast('Set A first, then B later in the song', true);
      player({ loop_ab: [S.loopA, pos], seek: S.loopA });
      S.loopA = null;
    } else {
      S.loopA = null;
      player({ loop_ab: [] });
    }
  })
);

// Tracks dialog: which hand plays each track.
$('#tracksbtn').addEventListener('click', () => S.song && $('#tracks').showModal());
export function renderTracks() {
  const list = $('#tracklist');
  list.innerHTML = '';
  S.song.tracks.forEach((t, i) => {
    if (!t.notes) return;
    const row = document.createElement('div');
    row.className = 'trackrow';
    row.innerHTML = `<span>${escapeHtml(t.name || 'Track ' + (i + 1))} <small>${t.notes} notes</small></span>`;
    const sel = document.createElement('select');
    sel.innerHTML = '<option value="right">Right hand</option><option value="left">Left hand</option>' +
      '<option value="split">Both hands (split at middle C)</option><option value="ignore">Ignore</option>';
    sel.value = t.role;
    sel.addEventListener('change', async () => {
      // At most one track per hand: anything this choice overlaps becomes ignored.
      const v = sel.value;
      const uses = (r) => (r === 'split' ? ['right', 'left'] : r === 'ignore' ? [] : [r]);
      const roles = S.song.tracks.map((tr, j) =>
        j === i ? v : uses(tr.role).some((h) => uses(v).includes(h)) ? 'ignore' : tr.role
      );
      await player({ track_roles: roles });
      S.song = null; // refetch with the new roles
    });
    row.append(sel);
    list.append(row);
  });
}

// Pops up when a run from the start reaches the end; the Pi keeps the records.
export let resultSeen; // JSON of the last result shown (undefined until the first update)
export async function showScorecard(r) {
  const key = JSON.stringify(r);
  if (resultSeen === undefined || key === resultSeen) return void (resultSeen = key); // don't pop up old results on load
  resultSeen = key;
  if (!r) return void ($('#card').hidden = true);
  const sec = Math.round(r.waited_ms / 1000);
  $('#card-stats').innerHTML =
    `<dt>Right</dt><dd>${r.right}</dd><dt>Wrong</dt><dd>${r.wrong}</dd>` +
    `<dt>Accuracy</dt><dd>${r.accuracy}%</dd><dt>Waited</dt><dd>${sec < 60 ? sec + ' s' : fmt(r.waited_ms)}</dd>`;
  $('#card-best').textContent = `At ${r.tempo}% tempo.`;
  $('#card').hidden = false;
  try {
    S.stats = await api('/api/stats');
    const best = S.stats[S.live.song];
    if (best) $('#card-best').textContent = `At ${r.tempo}% tempo. Your best: ${best.best_tempo}% tempo, ${best.best_accuracy}% right, finished ${best.finished}×.`;
    renderSongs();
  } catch {}
}
$('#card-close').addEventListener('click', () => ($('#card').hidden = true));
$('#card-again').addEventListener('click', () => {
  $('#card').hidden = true;
  player({ seek: -2000, playing: true });
});
