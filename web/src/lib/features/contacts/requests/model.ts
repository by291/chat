import type { FriendRequest, User } from '../api';

export type ContactRequest = FriendRequest & {
  direction: 'incoming' | 'outgoing';
  user: User;
};

/** Both directions share one chronological list while retaining action policy. */
export function mergeRequests(
  incoming: FriendRequest[],
  outgoing: FriendRequest[],
): ContactRequest[] {
  return [
    ...incoming.map((request) => ({
      ...request,
      direction: 'incoming' as const,
      user: request.sender,
    })),
    ...outgoing.map((request) => ({
      ...request,
      direction: 'outgoing' as const,
      user: request.recipient,
    })),
  ].sort(
    (left, right) =>
      Date.parse(right.created_at) - Date.parse(left.created_at) ||
      left.id.localeCompare(right.id),
  );
}
