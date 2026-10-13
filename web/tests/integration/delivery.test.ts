import { afterAll, beforeAll, expect, test } from 'vitest';
import {
  account,
  api,
  eventually,
  sdk,
  startIntegrationStack,
  startWorker,
  type IntegrationStack,
} from './support';
import { readFile } from 'node:fs/promises';
let stack: IntegrationStack;
beforeAll(async () => {
  stack = await startIntegrationStack();
});
afterAll(async () => {
  await stack?.cleanup();
});
test('committed messages survive a stopped worker and duplicate publication does not duplicate storage', async () => {
  const a = await account(stack);
  const b = await account(stack);
  const client = sdk(stack, b.cookie);
  const notifications: Record<string, unknown>[] = [];
  try {
    await client.connect();
    const sub = await client.subscribe(`personal:#${b.user.id}`);
    sub.on('publication', (ctx) => notifications.push(ctx.data));
    const conversation = await (
      await api(stack, a.cookie, '/conversations/direct', 'POST', {
        peer_id: b.user.id,
      })
    ).json();
    const key = crypto.randomUUID();
    const path = `/conversations/${conversation.id}/messages`;
    expect(
      (
        await api(stack, a.cookie, path, 'POST', {
          client_message_id: key,
          body: 'secret durable body',
        })
      ).status,
    ).toBe(201);
    expect(notifications).toHaveLength(0);
    expect(
      (await (await api(stack, b.cookie, path)).json()).items,
    ).toHaveLength(1);
    await startWorker(stack);
    await eventually(async () =>
      notifications.some((e) => e.kind === 'message.created'),
    );
    const envelope = notifications.find((e) => e.kind === 'message.created')!;
    expect(JSON.stringify(envelope)).not.toContain('secret durable body');
    await eventually(
      async () =>
        Number(
          (
            await stack.db.query(
              'SELECT count(*) FROM outbox WHERE completed_at IS NULL',
            )
          ).rows[0].count,
        ) === 0,
    );
    // Simulate the durable state after a worker died after publish but before acknowledgement.
    await stack.db.query(
      'UPDATE outbox SET completed_at=NULL,next_attempt_at=now() WHERE ordinal=2 AND aggregate_key=$1',
      [`conversation:${conversation.id}`],
    );
    await eventually(
      async () =>
        notifications.filter((e) => e.event_id === envelope.event_id).length >=
        2,
    );
    expect(
      (
        await api(stack, a.cookie, path, 'POST', {
          client_message_id: key,
          body: 'secret durable body',
        })
      ).status,
    ).toBe(200);
    expect(
      Number(
        (
          await stack.db.query(
            'SELECT count(*) FROM messages WHERE conversation_id=$1',
            [conversation.id],
          )
        ).rows[0].count,
      ),
    ).toBe(1);
  } finally {
    client.stop();
  }
});
test('a real worker crash after publish replays the leased head before later messages', async () => {
  // Stop the normal worker from the previous scenario, preserving the saved messages.
  if (stack.workerPid) {
    process.kill(stack.workerPid);
    stack.workerPid = null;
  }
  const a = await account(stack);
  const b = await account(stack);
  const client = sdk(stack, b.cookie);
  const seen: Record<string, unknown>[] = [];
  try {
    await client.connect();
    const sub = await client.subscribe(`personal:#${b.user.id}`);
    sub.on('publication', (ctx) => seen.push(ctx.data));
    const conv = await (
      await api(stack, a.cookie, '/conversations/direct', 'POST', {
        peer_id: b.user.id,
      })
    ).json();
    const key = crypto.randomUUID();
    const path = `/conversations/${conv.id}/messages`;
    expect(
      (
        await api(stack, a.cookie, path, 'POST', {
          client_message_id: key,
          body: 'survives crash',
        })
      ).status,
    ).toBe(201);
    await startWorker(stack, true);
    await eventually(async () => {
      try {
        await readFile(
          stack.envFile.replace('stack.env', 'published-marker.json'),
        );
        return true;
      } catch {
        return false;
      }
    });
    process.kill(stack.workerPid!);
    stack.workerPid = null;
    expect(seen.some((e) => e.kind === 'conversation.created')).toBe(true);
    expect(seen.some((e) => e.kind === 'message.created')).toBe(false);
    // Advance only this isolated test database lease to model the 30 second reclaim deadline.
    await stack.db.query(
      "UPDATE outbox SET lease_until=now()-interval '1 second' WHERE aggregate_key=$1",
      [`conversation:${conv.id}`],
    );
    await startWorker(stack);
    await eventually(async () =>
      seen.some((e) => e.kind === 'message.created'),
    );
    expect(
      seen.filter((e) => e.kind === 'conversation.created').length,
    ).toBeGreaterThanOrEqual(2);
    expect(
      (
        await api(stack, a.cookie, path, 'POST', {
          client_message_id: key,
          body: 'survives crash',
        })
      ).status,
    ).toBe(200);
    expect(
      Number(
        (
          await stack.db.query(
            'SELECT count(*) FROM messages WHERE conversation_id=$1',
            [conv.id],
          )
        ).rows[0].count,
      ),
    ).toBe(1);
  } finally {
    client.stop();
  }
}, 60000);
