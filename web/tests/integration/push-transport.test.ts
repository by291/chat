import { befriend } from './support';
import { expect, test } from 'vitest';
import { randomUUID } from 'node:crypto';

import { TestPushProvider } from './push-support';
import { TestRuntime } from './runtime';
import { account, api } from './support';

test('native worker sends an independently decrypted notification after a restart with stable VAPID keys', async () => {
  const provider = new TestPushProvider();
  let stack: TestRuntime | undefined;
  await provider.start();
  try {
    stack = await TestRuntime.start({ push: provider.environment() });
    const recipient = await account(stack);
    const sender = await account(stack);
    await befriend(stack, sender.cookie, recipient.cookie);
    const configuration = await (
      await api(stack, recipient.cookie, '/notifications/config')
    ).json();
    const registered = await api(
      stack,
      recipient.cookie,
      '/notifications/subscriptions',
      'POST',
      provider.subscription('native'),
    );
    expect(registered.status).toBe(201);
    const binding = await registered.json();
    const conversation = await (
      await api(stack, sender.cookie, '/conversations/direct', 'POST', {
        peer_id: recipient.user.id,
      })
    ).json();
    const message = await (
      await api(
        stack,
        sender.cookie,
        `/conversations/${conversation.id}/messages`,
        'POST',
        { client_message_id: randomUUID(), body: 'Private native message' },
      )
    ).json();

    await stack.startWorker();
    await expect
      .poll(() => provider.requests().length, { timeout: 20000 })
      .toBe(1);
    const request = provider.requests()[0];
    expect(request.vapidValid).toBe(true);
    expect(request.status).toBe(201);
    expect(request.payload).toMatchObject({
      schema_version: 1,
      generation: binding.generation,
      path: `/app/chats/${conversation.id}`,
    });
    expect(JSON.stringify(request.payload)).not.toContain(message.body);
    await expect
      .poll(
        async () =>
          (await stack!.db.query('SELECT status FROM push_jobs')).rows[0]
            .status,
      )
      .toBe('delivered');
    await stack.stopWorker();
    await stack.startWorker();

    expect(
      await (
        await api(stack, recipient.cookie, '/notifications/config')
      ).json(),
    ).toEqual(configuration);
    expect(
      (
        await stack.db.query(
          'SELECT count(*)::int AS count FROM push_subscriptions',
        )
      ).rows[0].count,
    ).toBe(1);
    expect(
      (await stack.db.query('SELECT status FROM push_jobs')).rows[0].status,
    ).toBe('delivered');
    expect(provider.requests()).toHaveLength(1);
    expect(
      (
        await stack.db.query(
          'SELECT last_read_message_seq FROM conversation_members WHERE user_id=$1',
          [recipient.user.id],
        )
      ).rows[0].last_read_message_seq,
    ).toBe('0');
  } finally {
    await stack?.cleanup();
    await provider.stop();
  }
}, 120000);

test('recovered pending push rechecks a revoked binding before provider dispatch', async () => {
  const provider = new TestPushProvider();
  let stack: TestRuntime | undefined;
  await provider.start();
  try {
    stack = await TestRuntime.start({ push: provider.environment() });
    const recipient = await account(stack);
    const sender = await account(stack);
    await befriend(stack, sender.cookie, recipient.cookie);
    const binding = await (
      await api(
        stack,
        recipient.cookie,
        '/notifications/subscriptions',
        'POST',
        provider.subscription('revoked'),
      )
    ).json();
    const conversation = await (
      await api(stack, sender.cookie, '/conversations/direct', 'POST', {
        peer_id: recipient.user.id,
      })
    ).json();
    await api(
      stack,
      sender.cookie,
      `/conversations/${conversation.id}/messages`,
      'POST',
      { client_message_id: randomUUID(), body: 'Retired message intent' },
    );
    expect(
      (
        await api(
          stack,
          recipient.cookie,
          `/notifications/subscriptions/${binding.id}`,
          'DELETE',
        )
      ).status,
    ).toBe(204);
    await stack.db.query(
      "UPDATE push_jobs SET not_before=clock_timestamp()-interval '1 second'",
    );

    await stack.startWorker();
    await expect
      .poll(
        async () =>
          (await stack!.db.query('SELECT status FROM push_jobs')).rows[0]
            .status,
      )
      .toBe('skipped');

    expect(provider.requests()).toEqual([]);
    expect(
      (
        await stack.db.query(
          'SELECT revoked_at IS NOT NULL AS revoked FROM push_subscriptions',
        )
      ).rows[0].revoked,
    ).toBe(true);
  } finally {
    await stack?.cleanup();
    await provider.stop();
  }
}, 120000);
