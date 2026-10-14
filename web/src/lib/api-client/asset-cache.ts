const CACHE_PREFIX = 'chat-pwa-assets:';
const PUBLIC_ASSETS = new Set([
  '/offline.html',
  '/manifest.webmanifest',
  '/theme.js',
  '/apple-touch-icon.png',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/maskable-192.png',
  '/icons/maskable-512.png',
]);

type AssetStore = {
  match(request: RequestInfo | URL): Promise<Response | undefined>;
  put(request: RequestInfo | URL, response: Response): Promise<void>;
};

type AssetStorage = {
  open(name: string): Promise<AssetStore>;
  keys(): Promise<string[]>;
  delete(name: string): Promise<boolean>;
};

type CacheOptions = {
  version: string;
  origin: string;
  assetPaths: string[];
  cacheStorage: AssetStorage;
  fetchAsset: typeof fetch;
};

/** Static transport is an allowlist, never a cache of arbitrary successful responses. */
export function createAssetCache(options: CacheOptions) {
  const origin = new URL(options.origin).origin;
  const name = CACHE_PREFIX + options.version;
  const urls = new Set(
    options.assetPaths
      .filter(
        (path) =>
          new URL(path, origin).pathname === path &&
          (PUBLIC_ASSETS.has(path) ||
            /^\/_app\/immutable\/[\w./-]+\.(?:js|css)$/.test(path)),
      )
      .map((path) => new URL(path, origin).href),
  );

  function handles(request: Request): boolean {
    const url = new URL(request.url);
    if (request.method !== 'GET' || url.origin !== origin) return false;
    return (
      urls.has(url.href) ||
      (request.mode === 'navigate' &&
        (url.pathname === '/' ||
          url.pathname === '/app' ||
          url.pathname.startsWith('/app/')))
    );
  }

  async function load(url: string): Promise<Response> {
    const response = await options.fetchAsset(url, {
      credentials: 'omit',
      redirect: 'error',
    });
    if (!response.ok || (response.url && response.url !== url)) {
      throw new Error('Static asset unavailable');
    }
    return response;
  }

  return {
    handles,
    async install(): Promise<void> {
      const cache = await options.cacheStorage.open(name);
      for (const url of urls) {
        await cache.put(url, await load(url));
      }
    },
    async activate(): Promise<void> {
      for (const key of await options.cacheStorage.keys()) {
        if (key.startsWith(CACHE_PREFIX) && key !== name) {
          await options.cacheStorage.delete(key);
        }
      }
    },
    async respond(request: Request): Promise<Response | undefined> {
      if (!handles(request)) return undefined;
      const url = new URL(request.url);
      if (urls.has(url.href)) {
        const cache = await options.cacheStorage.open(name);
        return (await cache.match(url.href)) ?? (await load(url.href));
      }
      if (
        request.mode === 'navigate' &&
        (url.pathname === '/' ||
          url.pathname === '/app' ||
          url.pathname.startsWith('/app/'))
      ) {
        try {
          return await options.fetchAsset(request);
        } catch {
          // Offline content contains no account records, and authenticated HTML
          // never enters CacheStorage. The network response is returned unchanged.
          const cache = await options.cacheStorage.open(name);
          return (
            (await cache.match(origin + '/offline.html')) ??
            new Response('暂时离线，请联网后重试。', {
              status: 503,
              headers: {
                'Content-Type': 'text/plain; charset=utf-8',
                'Cache-Control': 'no-store',
              },
            })
          );
        }
      }
      return undefined;
    },
  };
}

/** Worker composition uses this transport for untouched requests; no private caching occurs. */
export function fetchUncached(request: Request): Promise<Response> {
  return fetch(request);
}
