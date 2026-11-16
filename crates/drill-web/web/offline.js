const prefix = "drillforge-editor-" + self.registration.scope + ":";
const cacheName = prefix + "__BUILD_HASH__";
const files = [
  "./index.html",
  "./editor.css",
  "./editor.mjs",
  "./pkg/drill_web.js",
  "./pkg/drill_web_bg.wasm",
];
const urls = files.map((path) => new URL(path, self.registration.scope).href);

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(cacheName).then((cache) => cache.addAll(urls)));
});

self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    for (const key of await caches.keys()) {
      if (key.startsWith(prefix) && key !== cacheName) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  const scope = new URL(self.registration.scope);
  const isEditor = request.mode === "navigate" && url.origin === scope.origin &&
    (url.pathname === scope.pathname || url.pathname === new URL(urls[0]).pathname);
  if (!isEditor && !urls.includes(url.href)) return;
  event.respondWith((async () => {
    const cache = await caches.open(cacheName);
    return (await cache.match(isEditor ? urls[0] : request)) ?? fetch(request);
  })());
});
