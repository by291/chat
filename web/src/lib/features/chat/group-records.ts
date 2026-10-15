import type { QueryClient } from '@tanstack/svelte-query';

import { keys } from './cache';
import type { ChatApi, GroupMember } from './models';
import { sequence } from './models';

type RosterFlight = {
  membership: string;
  epoch: number;
  controller: AbortController;
  signal: AbortSignal;
  rerun: boolean;
  work: Promise<void>;
};

/** Owns authoritative roster cache writes; request identities never outlive a membership. */
export class GroupRecords {
  private flights = new Map<string, RosterFlight>();

  constructor(
    private options: {
      api: ChatApi;
      queryClient: QueryClient;
      userId: string;
      membership: (id: string) => string | undefined;
      active: () => boolean;
      epoch: () => number;
      failed: (message: string) => void;
    },
  ) {}

  members(id: string): GroupMember[] {
    return (
      this.options.queryClient.getQueryData<GroupMember[]>(keys.members(id)) ??
      []
    );
  }

  async load(id: string, signal?: AbortSignal) {
    const membership = this.options.membership(id);
    if (!membership || !this.options.active()) return;

    const previous = this.flights.get(id);
    const epoch = this.options.epoch();
    if (
      previous?.membership === membership &&
      previous.epoch === epoch &&
      !previous.signal.aborted
    ) {
      // A durable membership hint arriving during HTTP needs one fresh request
      // afterward; joining its existing promise alone would lose the invalidation.
      previous.rerun = true;
      return previous.work;
    }

    previous?.controller.abort();
    const controller = new AbortController();
    const flight: RosterFlight = {
      membership,
      epoch,
      controller,
      signal: signal
        ? AbortSignal.any([signal, controller.signal])
        : controller.signal,
      rerun: false,
      work: Promise.resolve(),
    };
    this.flights.set(id, flight);
    flight.work = this.refresh(id, flight);

    try {
      await flight.work;
    } finally {
      if (this.flights.get(id) === flight) this.flights.delete(id);
    }
  }

  private live(id: string, flight: RosterFlight) {
    return (
      this.flights.get(id) === flight &&
      !flight.signal.aborted &&
      this.options.active() &&
      this.options.epoch() === flight.epoch &&
      this.options.membership(id) === flight.membership
    );
  }

  private async refresh(id: string, flight: RosterFlight) {
    do {
      flight.rerun = false;
      if (!this.live(id, flight)) return;

      try {
        const rows = await this.options.api.members(id, flight.signal);
        const own = rows.find(
          (member) => member.user.id === this.options.userId,
        );

        // The roster identifies its own actor episode. A late response cannot
        // insert newly joined records into an older stream, or the reverse.
        if (
          this.live(id, flight) &&
          !flight.rerun &&
          own?.membership_id === flight.membership
        ) {
          this.options.queryClient.setQueryData<GroupMember[]>(
            keys.members(id),
            (current) =>
              rows.map((member) => {
                // Receipt replay can advance while HTTP is pending. Only the same
                // membership episode shares that monotonic cursor; a rejoin resets it.
                const existing = current?.find(
                  (record) =>
                    record.membership_id === member.membership_id &&
                    record.user.id === member.user.id,
                );

                return existing &&
                  sequence(existing.read_seq) > sequence(member.read_seq)
                  ? { ...member, read_seq: existing.read_seq }
                  : member;
              }),
          );
        }
      } catch (reason) {
        if (this.live(id, flight) && !flight.rerun) {
          this.options.queryClient.removeQueries({
            queryKey: keys.members(id),
          });
          this.options.failed(
            reason instanceof Error ? reason.message : '群成员加载失败，请重试',
          );
        }
      }
    } while (flight.rerun && this.live(id, flight));
  }

  receipt(id: string, membership: string, cursor: string) {
    this.options.queryClient.setQueryData<GroupMember[]>(
      keys.members(id),
      (rows) =>
        rows?.map((member) =>
          member.membership_id === membership
            ? {
                ...member,
                read_seq:
                  sequence(member.read_seq) > sequence(cursor)
                    ? member.read_seq
                    : cursor,
              }
            : member,
        ),
    );
  }

  forget(id: string) {
    this.flights.get(id)?.controller.abort();
    this.flights.delete(id);
    this.options.queryClient.removeQueries({ queryKey: keys.members(id) });
  }
}
