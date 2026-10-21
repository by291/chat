import type { components } from '#lib/api-client';

import { createGroupApi } from '../api';
import type { Group, GroupMember } from '../models';

/** UI permissions describe available controls; the server remains the authorization boundary. */
export function canManageMemberRole(
  group: Group,
  member: GroupMember,
): boolean {
  return group.my_role === 'owner' && member.role !== 'owner';
}

export function canRemoveGroupMember(
  group: Group,
  member: GroupMember,
  currentUserId: string,
): boolean {
  const manager = group.my_role === 'owner' || group.my_role === 'admin';
  return (
    manager &&
    member.user.id !== currentUserId &&
    member.role !== 'owner' &&
    (group.my_role === 'owner' || member.role === 'member')
  );
}

/** Coordinate group mutations and cache refreshes within the caller's captured membership episode. */
export class GroupActions {
  busy = $state(false);
  error = $state('');
  invitation = $state<components['schemas']['GroupInvitationView'] | null>(
    null,
  );

  private requests = createGroupApi();

  constructor(
    private options: {
      group: () => Group | undefined;
      membership: () => string | undefined;
      active: () => boolean;
      refreshSummaries: () => Promise<void>;
      refreshMembers: () => Promise<void>;
    },
  ) {}

  /** Cache/roster refreshes retain the dialog; only episode replacement clears its invitation. */
  resetPresentation() {
    this.invitation = null;
    this.error = '';
  }

  rename(title: string) {
    return this.mutate((conversationId, membershipId) =>
      this.requests.rename(conversationId, membershipId, title),
    );
  }

  setRole(userId: string, role: 'admin' | 'member') {
    return this.mutate((conversationId, membershipId) =>
      this.requests.role(conversationId, membershipId, userId, role),
    );
  }

  transferOwnership(userId: string) {
    return this.mutate((conversationId, membershipId) =>
      this.requests.transfer(conversationId, membershipId, userId),
    );
  }

  removeMember(userId: string) {
    return this.mutate((conversationId, membershipId) =>
      this.requests.remove(conversationId, membershipId, userId),
    );
  }

  async invite() {
    if (this.busy) return;

    // A previous single-use link must not remain copyable while a new one is pending.
    this.invitation = null;
    await this.mutate(async (conversationId, membershipId) => {
      const invitation = await this.requests.invite(
        conversationId,
        membershipId,
      );
      if (this.options.active() && this.options.membership() === membershipId) {
        this.invitation = invitation;
      }
    });
  }

  revokeInvitation() {
    return this.mutate(async (conversationId, membershipId) => {
      if (!this.invitation) return;

      await this.requests.revoke(
        conversationId,
        membershipId,
        this.invitation.invitation_id,
      );
      this.invitation = null;
    });
  }

  async leave(onLeft: () => Promise<void>) {
    const group = this.options.group();
    if (!group || this.busy) return;

    const membershipId = group.membership.membership_id;
    this.busy = true;

    try {
      await this.requests.leave(group.id, membershipId);
      const currentMembership = this.options.membership();
      // Removal may already have cleared the summary. A different current episode
      // instead belongs to a rejoin and must not be navigated away by this response.
      if (
        !this.options.active() ||
        (currentMembership && currentMembership !== membershipId)
      ) {
        return;
      }

      await this.options.refreshSummaries();
      await onLeft();
    } catch (reason) {
      this.error = reason instanceof Error ? reason.message : '退出失败';
    } finally {
      this.busy = false;
    }
  }

  private async mutate(
    operation: (
      conversationId: string,
      membershipId: string,
    ) => Promise<unknown>,
  ) {
    const group = this.options.group();
    if (!group || this.busy) return;

    const conversationId = group.id;
    const membershipId = group.membership.membership_id;
    this.busy = true;
    this.error = '';

    try {
      await operation(conversationId, membershipId);
      if (
        !this.options.active() ||
        this.options.membership() !== membershipId
      ) {
        return;
      }

      await this.options.refreshSummaries();
      await this.options.refreshMembers();
    } catch (reason) {
      this.error = reason instanceof Error ? reason.message : '群聊操作失败';
    } finally {
      this.busy = false;
    }
  }
}
