import {
  api as sharedApi,
  unwrap,
  type ApiClient,
  type components,
} from '../../api-client';
import type { ChatApi } from './models';

/** Typed chat HTTP operations; no component constructs an endpoint URL. */
export function createChatApi(api: ApiClient): ChatApi {
  return {
    list: (cursor, signal) =>
      unwrap(
        api.GET('/api/v1/conversations', {
          params: { query: { cursor, limit: 100 } },
          signal,
        }),
      ),
    conversation: (id, signal) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}', {
          params: { path: { id } },
          signal,
        }),
      ),
    snapshot: (id, before, signal, membership_id) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/messages', {
          params: { path: { id }, query: { before, limit: 50, membership_id } },
          signal,
        }),
      ),
    events: (id, after, signal, membership_id) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/events', {
          params: { path: { id }, query: { after, limit: 200, membership_id } },
          signal,
        }),
      ),
    send: (id, body) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/messages', {
          params: { path: { id } },
          body,
        }),
      ),
    edit: (id, message_id, body, membership_id) =>
      unwrap(
        api.PATCH('/api/v1/conversations/{id}/messages/{message_id}', {
          params: { path: { id, message_id } },
          body: { body, membership_id },
        }),
      ),
    withdraw: (id, message_id, membership_id) =>
      unwrap(
        api.DELETE('/api/v1/conversations/{id}/messages/{message_id}', {
          params: { path: { id, message_id }, query: { membership_id } },
        }),
      ),
    search: (id, q, before, signal, membership_id) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/search', {
          params: {
            path: { id },
            query: { q, before, limit: 50, membership_id },
          },
          signal,
        }),
      ),
    context: (id, message_id, signal, membership_id) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/messages/{message_id}/context', {
          params: {
            path: { id, message_id },
            query: { limit: 50, membership_id },
          },
          signal,
        }),
      ),
    setReaction: (id, message_id, kind, membership_id) =>
      unwrap(
        api.PUT('/api/v1/conversations/{id}/messages/{message_id}/reaction', {
          params: { path: { id, message_id } },
          body: { kind, membership_id },
        }),
      ),
    removeReaction: (id, message_id, membership_id) =>
      unwrap(
        api.DELETE(
          '/api/v1/conversations/{id}/messages/{message_id}/reaction',
          {
            params: { path: { id, message_id }, query: { membership_id } },
          },
        ),
      ),
    preferences: (id, patch, membership_id) =>
      unwrap(
        api.PATCH('/api/v1/conversations/{id}/preferences', {
          params: { path: { id } },
          body: { ...patch, membership_id },
        }),
      ),
    members: (id, signal) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/members', {
          params: { path: { id } },
          signal,
        }),
      ),
    read: (id, seq, membership_id) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/read', {
          params: { path: { id } },
          body: { message_seq: seq, membership_id },
        }),
      ),
    typing: async (id, is_typing, membership_id) => {
      await unwrap(
        api.POST('/api/v1/conversations/{id}/typing', {
          params: { path: { id } },
          body: { is_typing, membership_id },
        }),
      );
    },
    status: (id) =>
      unwrap(
        api.GET('/api/v1/users/{id}/status', { params: { path: { id } } }),
      ),
  };
}

/** Group mutations carry the caller's current episode; stale dialogs cannot use a rejoin. */
export function createGroupApi(api: ApiClient = sharedApi) {
  return {
    create: (title: string) =>
      unwrap(api.POST('/api/v1/conversations/groups', { body: { title } })),
    rename: (id: string, membership_id: string, title: string) =>
      unwrap(
        api.PATCH('/api/v1/conversations/{id}/group', {
          params: { path: { id } },
          body: { membership_id, title },
        }),
      ),
    role: (
      id: string,
      membership_id: string,
      user_id: string,
      role: 'admin' | 'member',
    ) =>
      unwrap(
        api.PATCH('/api/v1/conversations/{id}/members/{user_id}', {
          params: { path: { id, user_id } },
          body: { membership_id, role },
        }),
      ),
    remove: (id: string, membership_id: string, user_id: string) =>
      unwrap(
        api.DELETE('/api/v1/conversations/{id}/members/{user_id}', {
          params: { path: { id, user_id } },
          body: { membership_id },
        }),
      ),
    leave: (id: string, membership_id: string) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/leave', {
          params: { path: { id } },
          body: { membership_id },
        }),
      ),
    transfer: (id: string, membership_id: string, user_id: string) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/owner', {
          params: { path: { id } },
          body: { membership_id, user_id },
        }),
      ),
    invite: (id: string, membership_id: string) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/invitations', {
          params: { path: { id } },
          body: { membership_id },
        }),
      ),
    revoke: (id: string, membership_id: string, invitation_id: string) =>
      unwrap(
        api.DELETE('/api/v1/conversations/{id}/invitations/{invitation_id}', {
          params: { path: { id, invitation_id } },
          body: { membership_id },
        }),
      ),
    accept: (token: string) =>
      unwrap(api.POST('/api/v1/group-invitations/accept', { body: { token } })),
  };
}

export function findUser(username: string, api: ApiClient = sharedApi) {
  return unwrap(
    api.GET('/api/v1/users/by-username/{username}', {
      params: { path: { username } },
    }),
  );
}

export function startDirectConversation(
  peerId: string,
  api: ApiClient = sharedApi,
) {
  return unwrap(
    api.POST('/api/v1/conversations/direct', { body: { peer_id: peerId } }),
  );
}
