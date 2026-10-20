import {
  api as sharedApi,
  unwrap,
  type ApiClient,
  type components,
} from '#lib/api-client';

export type User = components['schemas']['UserView'];
export type Relationship = components['schemas']['RelationshipView'];
export type FriendRequest = components['schemas']['FriendRequestView'];
export type FriendsPage = components['schemas']['FriendsPage'];
export type RequestsPage = components['schemas']['FriendRequestsPage'];
export type RequestAction = components['schemas']['FriendRequestAction'];

/** Typed contacts operations; transport and endpoint construction stay in this adapter. */
export function createContactsApi(client: ApiClient = sharedApi) {
  return {
    friends: (q = '', cursor?: string, signal?: AbortSignal) =>
      unwrap(
        client.GET('/api/v1/friends', {
          params: { query: { q, cursor } },
          signal,
        }),
      ),
    requests: (
      direction: 'incoming' | 'outgoing',
      cursor?: string,
      signal?: AbortSignal,
    ) =>
      unwrap(
        client.GET('/api/v1/friend-requests', {
          params: { query: { direction, status: 'pending', cursor } },
          signal,
        }),
      ),
    relationship: (user_id: string, signal?: AbortSignal) =>
      unwrap(
        client.GET('/api/v1/friends/{user_id}', {
          params: { path: { user_id } },
          signal,
        }),
      ),
    request: (user_id: string, signal?: AbortSignal) =>
      unwrap(
        client.POST('/api/v1/friend-requests', { body: { user_id }, signal }),
      ),
    respond: (id: string, action: RequestAction, signal?: AbortSignal) =>
      unwrap(
        client.PATCH('/api/v1/friend-requests/{id}', {
          params: { path: { id } },
          body: { action },
          signal,
        }),
      ),
    remove: (user_id: string, signal?: AbortSignal) =>
      unwrap(
        client.DELETE('/api/v1/friends/{user_id}', {
          params: { path: { user_id } },
          signal,
        }),
      ),
    searchUsers: (q: string, signal?: AbortSignal) =>
      unwrap(
        client.GET('/api/v1/users/search', {
          params: { query: { q } },
          signal,
        }),
      ),
  };
}
