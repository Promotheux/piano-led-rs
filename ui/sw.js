// Minimal service worker: lets Chrome install the app. Everything is served
// live by the Pi (no offline cache), so the app always matches the engine.
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', () => {});
