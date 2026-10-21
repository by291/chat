import type { Conversation, Message } from '../models';
import { isGroup, sequence } from '../models';

const MAX_LIVE_CANDIDATES = 200;

/** Tracks exact live hints separately from canonical messages and replay cursors.
 * Startup/reconnect floors silence backlog; unknown hints wait for authorized
 * metadata rather than promoting an entire replay page into notifications.
 */
export class LiveMessageCandidates {
  private candidates = new Set<string>();
  private floors = new Map<string, bigint>();

  constructor(
    private options: {
      conversations: () => Conversation[];
      messages: (id: string) => Message[];
      applied?: (message: Message, fresh: boolean) => void;
    },
  ) {}

  floor(id: string) {
    return this.floors.get(id) ?? 0n;
  }

  hint(id: string, eventSeq: string, applied: string, lastRead: bigint) {
    const candidateSequence = sequence(eventSeq);
    if (candidateSequence <= this.floor(id)) return;

    if (candidateSequence <= sequence(applied)) {
      const conversation = this.options
        .conversations()
        .find((record) => record.id === id);
      const message = this.options
        .messages(id)
        .find((record) => record.created_seq === eventSeq);

      // A preceding receipt may replay a message before its own publication.
      // Only this exact authorized hint may announce it, and a cursor already
      // read elsewhere must remain silent.
      if (
        message &&
        conversation &&
        sequence(conversation.my_read_seq) < sequence(message.created_seq) &&
        lastRead < sequence(message.created_seq)
      ) {
        this.options.applied?.(message, true);
      }
      return;
    }

    this.candidates.add(`${id}/${eventSeq}`);
    if (this.candidates.size > MAX_LIVE_CANDIDATES) {
      this.candidates.delete(this.candidates.values().next().value!);
    }
  }

  take(id: string, eventSeq: string) {
    return this.candidates.delete(`${id}/${eventSeq}`);
  }

  first(id: string) {
    const conversation = this.options
      .conversations()
      .find((record) => record.id === id);
    const joined = isGroup(conversation)
      ? sequence(conversation.membership.joined_seq)
      : 0n;
    let first: bigint | undefined;

    for (const candidate of this.candidates) {
      if (!candidate.startsWith(`${id}/`)) continue;

      const candidateSequence = sequence(candidate.slice(id.length + 1));
      if (candidateSequence < joined || candidateSequence <= this.floor(id)) {
        this.candidates.delete(candidate);
        continue;
      }
      if (first === undefined || candidateSequence < first) {
        first = candidateSequence;
      }
    }

    return first;
  }

  baseline(id: string, snapshotSequence: string) {
    const first = this.first(id);
    return first !== undefined && first <= sequence(snapshotSequence)
      ? first - 1n
      : sequence(snapshotSequence);
  }

  /** Freeze known history only after startup/reconnect reconciliation. */
  captureBaseline() {
    this.candidates.clear();
    for (const conversation of this.options.conversations()) {
      this.floors.set(conversation.id, sequence(conversation.latest_event_seq));
    }
  }

  clear() {
    this.candidates.clear();
  }

  forget(id: string) {
    this.floors.delete(id);
    for (const candidate of this.candidates) {
      if (candidate.startsWith(`${id}/`)) this.candidates.delete(candidate);
    }
  }

  stop() {
    this.candidates.clear();
    this.floors.clear();
  }
}
