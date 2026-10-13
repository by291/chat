import { api, unwrap } from '../../api-client';
import type { MediaApi } from './models';

/** Media authorization stays on the application origin; transfers use separate signed grants. */
export function createMediaApi(): MediaApi {
  return {
    create: (body, signal) =>
      unwrap(api.POST('/api/v1/media/uploads', { body, signal })),
    renew: (id, signal) =>
      unwrap(
        api.POST('/api/v1/media/{id}/upload-url', {
          params: { path: { id } },
          signal,
        }),
      ),
    complete: (id, attempt_id, signal) =>
      unwrap(
        api.POST('/api/v1/media/{id}/complete', {
          params: { path: { id } },
          body: { attempt_id },
          signal,
        }),
      ),
    status: (id, signal) =>
      unwrap(
        api.GET('/api/v1/media/{id}', { params: { path: { id } }, signal }),
      ),
    cancel: async (id) => {
      await unwrap(
        api.DELETE('/api/v1/media/{id}', { params: { path: { id } } }),
      );
    },
    download: (id, variant, signal) =>
      unwrap(
        api.GET('/api/v1/media/{id}/download', {
          params: { path: { id }, query: { variant } },
          signal,
        }),
      ),
  };
}

/** Consume a signed response with no cookies and no persistent URL record. */
export async function downloadBlob(
  grant: { url: string; headers: Record<string, string> },
  signal: AbortSignal,
) {
  const response = await fetch(grant.url, {
    headers: grant.headers,
    credentials: 'omit',
    signal,
    cache: 'no-store',
  });
  if (!response.ok) throw new Error('媒体下载失败，请重试');
  return response.blob();
}
