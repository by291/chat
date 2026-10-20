import { api, unwrap, ApiFailure } from '#lib/api-client';

/** Settings persistence is shared by forms, without coupling them to HTTP paths. */
export const settingsApi = {
  updateProfile: (displayName: string, signal?: AbortSignal) =>
    unwrap(
      api.PATCH('/api/v1/users/me', {
        body: { display_name: displayName },
        signal,
      }),
    ),
  sessions: (signal?: AbortSignal) =>
    unwrap(api.GET('/api/v1/sessions', { signal })),
  revokeSession: (id: string) =>
    unwrap(api.DELETE('/api/v1/sessions/{id}', { params: { path: { id } } })),
  setAvatar: (attachment_id: string) =>
    unwrap(api.PUT('/api/v1/users/me/avatar', { body: { attachment_id } })),
  clearAvatar: () => unwrap(api.DELETE('/api/v1/users/me/avatar')),
  avatar: async (id: string, signal?: AbortSignal) => {
    try {
      return await unwrap(
        api.GET('/api/v1/users/{id}/avatar', {
          params: { path: { id } },
          signal,
        }),
      );
    } catch (error) {
      if (error instanceof ApiFailure && error.status === 404) return null;
      throw error;
    }
  },
};
