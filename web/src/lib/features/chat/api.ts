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
    snapshot: (id, before, signal) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/messages', {
          params: { path: { id }, query: { before, limit: 50 } },
          signal,
        }),
      ),
    events: (id, after, signal) =>
      unwrap(
        api.GET('/api/v1/conversations/{id}/events', {
          params: { path: { id }, query: { after, limit: 200 } },
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
    read: (id, seq) =>
      unwrap(
        api.POST('/api/v1/conversations/{id}/read', {
          params: { path: { id } },
          body: { message_seq: seq },
        }),
      ),
    typing: async (id, is_typing) => {
      await unwrap(
        api.POST('/api/v1/conversations/{id}/typing', {
          params: { path: { id } },
          body: { is_typing },
        }),
      );
    },
    status: (id) =>
      unwrap(
        api.GET('/api/v1/users/{id}/status', { params: { path: { id } } }),
      ),
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
