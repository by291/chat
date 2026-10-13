import { api, unwrap } from '../../api-client';

/** Settings persistence is shared by forms, without coupling them to HTTP paths. */
export const settingsApi = {
  updateProfile: (displayName: string) =>
    unwrap(
      api.PATCH('/api/v1/users/me', { body: { display_name: displayName } }),
    ),
  sessions: () => unwrap(api.GET('/api/v1/sessions')),
  revokeSession: (id: string) =>
    unwrap(api.DELETE('/api/v1/sessions/{id}', { params: { path: { id } } })),
};
