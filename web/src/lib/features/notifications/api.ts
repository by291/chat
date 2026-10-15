import { api, unwrap, type ApiClient, type components } from '#lib/api-client';

export type PushBinding = components['schemas']['SubscriptionBinding'];
export type PushActivity = components['schemas']['ActivityInput'];
export type PushInput = components['schemas']['SubscriptionInput'];

const REQUEST_DEADLINE_MS = 5_000;

/** Optional push requests have a short deadline; a lost activity hint expires on the server. */
async function bounded<T>(
  request: (signal: AbortSignal) => Promise<T>,
  parent?: AbortSignal,
): Promise<T> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  parent?.addEventListener('abort', abort, { once: true });
  if (parent?.aborted) controller.abort();
  const deadline = setTimeout(abort, REQUEST_DEADLINE_MS);

  try {
    return await request(controller.signal);
  } finally {
    clearTimeout(deadline);
    parent?.removeEventListener('abort', abort);
  }
}

/** Typed HTTP is confined here; capability material never enters chat records or storage. */
export function createNotificationApi(client: ApiClient = api) {
  return {
    config: (signal?: AbortSignal) =>
      bounded(
        (signal) =>
          unwrap(client.GET('/api/v1/notifications/config', { signal })),
        signal,
      ),
    register: (body: PushInput, signal?: AbortSignal) =>
      bounded(
        (signal) =>
          unwrap(
            client.POST('/api/v1/notifications/subscriptions', {
              body,
              signal,
            }),
          ),
        signal,
      ),
    remove: async (id: string, signal?: AbortSignal): Promise<void> => {
      await bounded(
        (signal) =>
          unwrap(
            client.DELETE('/api/v1/notifications/subscriptions/{id}', {
              params: { path: { id } },
              signal,
            }),
          ),
        signal,
      );
    },
    activity: async (
      id: string,
      body: PushActivity,
      signal?: AbortSignal,
    ): Promise<void> => {
      await bounded(
        (signal) =>
          unwrap(
            client.POST('/api/v1/notifications/subscriptions/{id}/activity', {
              params: { path: { id } },
              body,
              signal,
              keepalive: !body.visible || !body.focused,
            }),
          ),
        signal,
      );
    },
  };
}

export type NotificationApi = ReturnType<typeof createNotificationApi>;
