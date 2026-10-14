import type { Message, Reply } from './models';
import { sequence } from './models';

function latestReply(
  left: Reply | null | undefined,
  right: Reply | null | undefined,
) {
  if (!left) return right;
  if (!right) return left;
  return sequence(left.revision_seq) > sequence(right.revision_seq)
    ? left
    : right;
}

/** All HTTP/replay sources share this merge; nested references have independent revisions.
 * A withdrawn target must stay withdrawn even when an old reply page arrives later.
 */
export function mergeMessageRecords(
  existing: Message[],
  incoming: Message[],
): Message[] {
  const records = new Map(existing.map((record) => [record.id, record]));
  for (const record of incoming) {
    const previous = records.get(record.id);
    const current =
      previous &&
      sequence(previous.revision_seq) > sequence(record.revision_seq)
        ? previous
        : record;
    records.set(
      record.id,
      current.withdrawn
        ? current
        : {
            ...current,
            reply: latestReply(previous?.reply, record.reply),
          },
    );
  }

  // A preview can be newer than an unloaded target, and its enclosing message's
  // revision does not change when the target changes. Keep the newest reference
  // seen anywhere in this authorized cache, then refresh every matching preview.
  const references = new Map<string, Reply>();
  for (const record of records.values()) {
    const reference: Reply = {
      id: record.id,
      sender_id: record.sender_id,
      sender_name: record.sender_name,
      created_seq: record.created_seq,
      revision_seq: record.revision_seq,
      body: [...record.body].slice(0, 160).join(''),
      withdrawn: record.withdrawn,
    };
    references.set(
      record.id,
      latestReply(references.get(record.id), reference)!,
    );
    if (record.reply)
      references.set(
        record.reply.id,
        latestReply(references.get(record.reply.id), record.reply)!,
      );
  }

  return [...records.values()]
    .map((record) => {
      const target = references.get(record.id);
      // A withdrawn preview is an authorized terminal state, even if the full
      // target was loaded earlier. Edited previews remain truncated and cannot
      // replace a full body; only their withdrawal flag authorizes this removal.
      if (
        target?.withdrawn &&
        sequence(target.revision_seq) > sequence(record.revision_seq)
      ) {
        return {
          ...record,
          revision_seq: target.revision_seq,
          withdrawn: true,
          body: '',
          attachments: [],
          reply: undefined,
        };
      }
      if (record.withdrawn || !record.reply) return record;
      const reply = references.get(record.reply.id);
      return reply ? { ...record, reply } : record;
    })
    .sort((left, right) =>
      sequence(left.created_seq) < sequence(right.created_seq)
        ? -1
        : sequence(left.created_seq) > sequence(right.created_seq)
          ? 1
          : 0,
    );
}
