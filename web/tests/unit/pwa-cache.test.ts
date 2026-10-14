import { expect, test } from 'vitest';

import { createAssetCache } from '../../src/lib/api-client/asset-cache';

const origin = 'https://chat.example.test';
const immutable = '/_app/immutable/chunks/app.abc.js';
const allowed = [immutable, '/offline.html', '/icons/icon-192.png'];

test('asset paths cannot use dot segments to escape the immutable directory', async () => {
  const fixture = storageFixture();
  const cache = createAssetCache({
    origin,
    version: 'one',
    assetPaths: [immutable, '/_app/immutable/../../api/v1/auth/me.js'],
    cacheStorage: fixture.storage,
    fetchAsset: async () => new Response('static'),
  });

  await cache.install();

  expect([...[...fixture.stores.values()][0].keys()]).toEqual([
    origin + immutable,
  ]);
});

function storageFixture() {
  const stores = new Map<string, Map<string, Response>>();
  const deleted: string[] = [];
  const storage = {
    async keys() {
      return [...stores.keys()];
    },
    async delete(name: string) {
      deleted.push(name);
      return stores.delete(name);
    },
    async open(name: string) {
      let store = stores.get(name);
      if (!store) {
        store = new Map();
        stores.set(name, store);
      }
      const records = store;
      return {
        async match(request: RequestInfo | URL) {
          return records.get(String(request))?.clone();
        },
        async put(request: RequestInfo | URL, response: Response) {
          records.set(String(request), response.clone());
        },
      };
    },
  };

  return { storage, stores, deleted };
}

test('installation stores only approved static assets and private or arbitrary paths always bypass', async () => {
  const fixture = storageFixture();
  const cache = createAssetCache({
    origin,
    version: 'one',
    assetPaths: [...allowed, '/api/v1/auth/me', '/app', '/media/secret'],
    cacheStorage: fixture.storage,
    fetchAsset: async () => new Response('static'),
  });

  await cache.install();

  const savedKeys = [...[...fixture.stores.values()][0].keys()];
  expect(savedKeys).toEqual(allowed.map((path) => origin + path));
  for (const path of [
    '/api/v1/auth/me',
    '/health/ready',
    '/connection/websocket',
    '/media/file?signature=private',
    '/unlisted.js',
    '/verify-email#token=private',
    '/reset-password#token=private',
  ]) {
    const request = new Request(origin + path);
    expect(cache.handles(request)).toBe(false);
    expect(await cache.respond(request)).toBeUndefined();
  }
  expect(
    await cache.respond(new Request(origin + immutable, { method: 'POST' })),
  ).toBeUndefined();
  expect(
    await cache.respond(new Request('https://media.example.test' + immutable)),
  ).toBeUndefined();
});

test('listed immutable assets are available offline without caching successful API responses', async () => {
  const fixture = storageFixture();
  let online = true;
  const cache = createAssetCache({
    origin,
    version: 'one',
    assetPaths: allowed,
    cacheStorage: fixture.storage,
    fetchAsset: async () => {
      if (!online) throw new Error('Offline');
      return new Response('immutable bytes');
    },
  });
  await cache.install();
  online = false;

  expect(
    await (await cache.respond(new Request(origin + immutable)))?.text(),
  ).toBe('immutable bytes');
  expect(
    await cache.respond(new Request(origin + '/api/v1/conversations')),
  ).toBeUndefined();
});

test('activation removes only obsolete application asset caches', async () => {
  const fixture = storageFixture();
  fixture.stores.set('another-application', new Map());
  const first = createAssetCache({
    origin,
    version: 'one',
    assetPaths: allowed,
    cacheStorage: fixture.storage,
    fetchAsset: async () => new Response('old'),
  });
  const second = createAssetCache({
    origin,
    version: 'two',
    assetPaths: allowed,
    cacheStorage: fixture.storage,
    fetchAsset: async () => new Response('new'),
  });
  await first.install();
  await second.install();

  await second.activate();

  expect(fixture.stores.has('another-application')).toBe(true);
  expect(fixture.deleted).toHaveLength(1);
  expect(
    await (await second.respond(new Request(origin + immutable)))?.text(),
  ).toBe('new');
});

test('offline app navigation returns generic content and never a cached authenticated page', async () => {
  const fixture = storageFixture();
  let online = true;
  const cache = createAssetCache({
    origin,
    version: 'one',
    assetPaths: ['/offline.html'],
    cacheStorage: fixture.storage,
    fetchAsset: async () => {
      if (!online) throw new Error('Offline');
      return new Response('Generic offline page');
    },
  });
  await cache.install();
  online = false;
  const navigation = new Request(origin + '/app');
  Object.defineProperty(navigation, 'mode', { value: 'navigate' });

  expect(await (await cache.respond(navigation))?.text()).toBe(
    'Generic offline page',
  );
  expect([...fixture.stores.values()][0].has(origin + '/app')).toBe(false);
});

test('installation rejects a listed asset redirected to a private response', async () => {
  const fixture = storageFixture();
  const response = new Response('private identity');
  Object.defineProperty(response, 'url', { value: origin + '/api/v1/auth/me' });
  const cache = createAssetCache({
    origin,
    version: 'one',
    assetPaths: [immutable],
    cacheStorage: fixture.storage,
    fetchAsset: async () => response,
  });

  await expect(cache.install()).rejects.toThrow();

  expect([...fixture.stores.values()][0].size).toBe(0);
});
