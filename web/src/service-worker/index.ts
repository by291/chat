import { self } from '$app/service-worker';
import { version } from '$app/env';
import { assets, immutable } from '$app/manifest';

import { createAssetCache, fetchUncached } from '../lib/api-client/asset-cache';
import { showPush, openNotification } from './push';

const cache = createAssetCache({
  version,
  origin: self.location.origin,
  assetPaths: [...immutable, ...assets].map(
    ({ path }) => '/' + path.replace(/^\//, ''),
  ),
  cacheStorage: self.caches,
  fetchAsset: self.fetch.bind(self),
});

self.addEventListener('install', (event) => event.waitUntil(cache.install()));
self.addEventListener('activate', (event) => event.waitUntil(cache.activate()));
self.addEventListener('fetch', (event) => {
  // Returning before respondWith preserves native API/media/XHR semantics;
  // an unlisted request never enters a worker-owned fetch or cache operation.
  if (!cache.handles(event.request)) return;
  event.respondWith(
    cache
      .respond(event.request)
      .then((response) => response ?? fetchUncached(event.request)),
  );
});
self.addEventListener('message', (event) => {
  // Only the explicit UI update action asks a waiting worker to replace its predecessor.
  // Installation never claims clients or interrupts an authenticated page automatically.
  if (event.data?.type === 'activate-update')
    event.waitUntil(self.skipWaiting());
});

self.addEventListener('push', (event) => {
  let payload: unknown;
  try {
    payload = event.data?.json();
  } catch {
    // Invalid provider data still requires a visible generic notification.
    payload = undefined;
  }
  event.waitUntil(showPush(self.registration, payload));
});
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  event.waitUntil(
    openNotification(
      self.clients,
      event.notification.data,
      self.location.origin,
    ),
  );
});
