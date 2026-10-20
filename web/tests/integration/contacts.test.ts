import { test, expect } from 'vitest';

import {
  account,
  api,
  eventually,
  sdk,
  startIntegrationStack,
} from './support';

test('contacts hints reach only participants and durable retries safely repeat invalidation', async () => {
  const stack = await startIntegrationStack();
  const alice = await account(stack);
  const bob = await account(stack);
  const outsider = await account(stack);
  const clients = [alice, bob, outsider].map((user) => sdk(stack, user.cookie));
  const received: unknown[][] = [[], [], []];
  try {
    for (const [index, client] of clients.entries()) {
      await client.connect();
      const subscription = await client.subscribe(
        `personal:#${[alice, bob, outsider][index].user.id}`,
      );
      subscription.on('publication', (event) =>
        received[index].push(event.data),
      );
    }
    const response = await api(
      stack,
      alice.cookie,
      '/friend-requests',
      'POST',
      { user_id: bob.user.id },
    );
    expect(response.status).toBe(200);
    const request = await response.json();
    expect(received).toEqual([[], [], []]);
    await stack.startWorker();
    await eventually(
      async () => received[0].length === 1 && received[1].length === 1,
    );
    expect(received).toEqual([
      [{ kind: 'contacts.updated' }],
      [{ kind: 'contacts.updated' }],
      [],
    ]);
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
    // Model a successful publish whose completion acknowledgement was lost.
    await stack.db.query(
      "UPDATE outbox SET completed_at=NULL, next_attempt_at=now() WHERE payload->>'type'='contacts.updated'",
    );
    await eventually(
      async () => received[0].length >= 2 && received[1].length >= 2,
    );
    expect(
      (
        await (
          await api(stack, bob.cookie, '/friend-requests?direction=incoming')
        ).json()
      ).pending_incoming_count,
    ).toBe(1);
    expect(
      (
        await api(
          stack,
          bob.cookie,
          `/friend-requests/${request.id}`,
          'PATCH',
          { action: 'decline' },
        )
      ).status,
    ).toBe(200);
    await eventually(
      async () => received[0].length >= 3 && received[1].length >= 3,
    );
    expect(received[2]).toEqual([]);
    expect(
      (
        await stack.db.query(
          'SELECT count(*)::int AS count FROM conversation_events',
        )
      ).rows[0].count,
    ).toBe(0);
  } finally {
    clients.forEach((client) => client.stop());
    await stack.cleanup();
  }
}, 60000);
