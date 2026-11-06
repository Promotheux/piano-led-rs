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
export const prettyName = (n) => n.replace(/\.midi?$/i, '').replace(/[_]+/g, ' ');
/** A library entry's title (from its sidecar), else its prettified file name. */
export const songTitle = (s) => s.title || prettyName(s.name);

export function setSeg(sel, value) {
  $$(`${sel} button`).forEach((b) => b.classList.toggle('on', b.dataset.v === value));
}
export const escapeHtml = (s) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
