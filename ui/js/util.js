// Small helpers: DOM, HTTP, toasts, formatting.

// Piano LED tablet app. The Pi runs the player; this page draws it and sends commands.

export const $ = (s) => document.querySelector(s);
export const $$ = (s) => [...document.querySelectorAll(s)]; // last settings generation seen from the Pi

export async function api(path, opts = {}) {
  const res = await fetch(path, opts);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || res.statusText);
  return body;
}
export const post = (path, data) =>
  api(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
export const player = (cmd) => post('/api/player', cmd).catch((e) => toast(e.message, true));

export let toastTimer;
export function toast(msg, error = false) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = 'show' + (error ? ' error' : '');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.className = ''), 2500);
}
export const fmt = (ms) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
};
// Drawn icons, one line style, so every device shows the same thing (emoji
// and symbol characters render differently, or not at all, per platform).
export const ICON = {
  play: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10.5-6.5z" fill="currentColor"/></svg>',
  pause: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="5.5" width="3.5" height="13" rx="1" fill="currentColor"/><rect x="13.5" y="5.5" width="3.5" height="13" rx="1" fill="currentColor"/></svg>',
  restart: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 6v12" /><path d="M18 6.5v11L10 12z" fill="currentColor" stroke="none"/></svg>',
  edit: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 19.5l1-4L15.5 5.5a2.1 2.1 0 0 1 3 3l-10 10z"/><path d="M13.5 7.5l3 3"/></svg>',
  trash: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 7h15"/><path d="M9.5 7V5h5v2"/><path d="M6.5 7l1 12h9l1-12"/><path d="M10.5 11v5M13.5 11v5"/></svg>',
  fullscreen: '<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 9V4.5H9M15 4.5h4.5V9M19.5 15v4.5H15M9 19.5H4.5V15"/></svg>',
};
export const prettyName = (n) => n.replace(/\.midi?$/i, '').replace(/[_]+/g, ' ');
/** A library entry's title (from its sidecar), else its prettified file name. */
export const songTitle = (s) => s.title || prettyName(s.name);

export function setSeg(sel, value) {
  $$(`${sel} button`).forEach((b) => b.classList.toggle('on', b.dataset.v === value));
}
export const escapeHtml = (s) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
