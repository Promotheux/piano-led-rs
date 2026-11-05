// Tabs and views.
import { $, $$ } from './util.js';
import { resize } from './view.js';

export function show(view) {
  const tab = $(`#${view}`).dataset.tab || view; // app views keep the Apps tab lit
  $$('.tab').forEach((b) => b.classList.toggle('active', b.dataset.view === tab));
  $$('.view').forEach((v) => v.classList.toggle('active', v.id === view));
  if (view === 'player') resize();
}
$$('.tab').forEach((b) => b.addEventListener('click', () => show(b.dataset.view)));
$$('[data-open]').forEach((b) => b.addEventListener('click', () => show(b.dataset.open)));
$('#fullscreen').addEventListener('click', () => {
  if (document.fullscreenElement) document.exitFullscreen();
  else document.documentElement.requestFullscreen?.().catch(() => {});
});
